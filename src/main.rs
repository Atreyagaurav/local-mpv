use clap::Parser;
use std::fs;
use std::io::prelude::*;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::exit;

use arboard::Clipboard;
use libmpv::{events::Event, FileState, Mpv};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// track clipboard contents to play media
    #[arg(short, long)]
    clipboard: bool,
    /// run mpv in loop mode
    #[arg(short, long)]
    r#loop: bool,
    /// add things in playlist instead of playing it instantly
    #[arg(short, long)]
    append: bool,
    /// Do not show video play audio only
    #[arg(short, long)]
    no_video: bool,
    /// Fullscreen
    #[arg(short, long, conflicts_with = "no_video")]
    fullscreen: bool,
    /// Run a server to control the mpv
    #[arg(short, long)]
    server: bool,
    /// Run the server in the given port
    #[arg(short, long, default_value = "6780")]
    port: u16,
    /// Display QR code for URL
    #[arg(short, long, requires = "server")]
    qr: bool,
    /// Files to play by MPV
    files: Vec<PathBuf>,

    /// Options for the mpv, only key-value pairs and bool flags are accepted
    ///
    /// --idle option cannot be changed.
    #[arg(num_args(0..), last(true))]
    options: Vec<String>,
}

fn parse_options(options: &[String]) -> Vec<(&str, &str)> {
    let mut seekval = false;
    let mut lastarg = "";
    let mut args = Vec::new();
    for op in options {
        if op.starts_with("--") {
            if seekval {
                args.push((lastarg, "yes"));
            }

            if let Some((k, v)) = op[2..].split_once("=") {
                args.push((k, v));
                seekval = false;
            } else {
                lastarg = &op[2..];
                seekval = true;
            }
        }
    }
    args
}

fn main() -> libmpv::Result<()> {
    let args = Args::parse();
    let mut ctx = Clipboard::new().unwrap();
    let mut clip_txt = ctx.get_text().unwrap_or_else(|_| String::from(""));

    let mpv = Mpv::new()?;
    mpv.set_property("osc", "yes")?;
    mpv.set_property("input-default-bindings", "yes")?;
    mpv.set_property("input-vo-keyboard", "yes")?;
    mpv.set_property("input-media-keys", "yes")?;
    if args.no_video {
        mpv.set_property("vid", "no")?;
    } else {
        mpv.set_property("geometry", "400-0-20")?;
    }
    if args.fullscreen {
        mpv.set_property("fullscreen", true)?;
    }
    if args.r#loop {
        mpv.set_property("loop-playlist", "inf")?;
    }
    let mut ev_ctx = mpv.create_event_context();

    for (k, v) in parse_options(&args.options) {
        mpv.set_property(k, v)?;
    }
    mpv.set_property("idle", "yes")?;

    let files: Vec<(&str, FileState, Option<&str>)> = args
        .files
        .iter()
        .filter_map(|f| f.to_str())
        .map(|f| (f, FileState::AppendPlay, None))
        .collect();
    mpv.playlist_load_files(&files)?;

    std::thread::scope(|s| {
        if args.clipboard {
            s.spawn(|| loop {
                let clip_new = ctx.get_text().unwrap_or_else(|_| String::from(""));

                if clip_new != clip_txt {
                    // let url = url::Url::parse(&clip_new);
                    // println!("{:?}", url);
                    // // url fails on file path, Path::from never fails
                    // if url.is_ok() {
                    println!("{}", clip_new);
                    mpv.playlist_load_files(&[(
                        &clip_new,
                        if args.append {
                            FileState::AppendPlay
                        } else {
                            FileState::Replace
                        },
                        None,
                    )])
                    .unwrap();
                    mpv.unpause().ok();
                    // }
                    clip_txt = clip_new;
                }
                std::thread::sleep(std::time::Duration::from_millis(100));
            });
        }

        s.spawn(|| loop {
            let ev = ev_ctx.wait_event(600.).unwrap_or(Err(libmpv::Error::Null));
            match ev {
                Ok(Event::Shutdown) => {
                    exit(0);
                }
                _ => (),
            }
        });

        if args.server {
            s.spawn(|| {
                let addr = if_addrs::get_if_addrs()
                    .unwrap()
                    .into_iter()
                    .map(|a| a.ip())
                    .filter(|a| a.is_ipv4() && !a.to_string().contains("127.0.0.1"))
                    .next()
                    .unwrap();
                let hp_addr = format!("http://{addr}:{}", args.port);
                println!("{}", hp_addr);
                if args.qr {
                    fast_qr::QRBuilder::new(hp_addr).build().unwrap().print();
                }
                let endpoint = format!("{addr}:{}", args.port);
                let listener = TcpListener::bind(endpoint).unwrap();
                for incoming_stream in listener.incoming() {
                    let mut stream = incoming_stream.unwrap();
                    handle_connection(&mut stream, &mpv);
                }
            });
        }
    });
    Ok(())
}

fn handle_connection(stream: &mut TcpStream, mpv: &Mpv) {
    // Buffer to read the incoming request
    let mut buffer = [0; 1024];
    stream.read(&mut buffer).unwrap();

    // Convert the request buffer to a string
    let request_str = String::from_utf8_lossy(&buffer);
    if request_str.starts_with("GET") {
        let filepath = request_str
            .split_whitespace()
            .nth(1)
            .unwrap_or("/")
            .to_string();
        serve_requested_file(&filepath, stream);
    } else if request_str.starts_with("POST") {
        let path = request_str
            .split_whitespace()
            .nth(1)
            .unwrap_or("/")
            .to_string();
        handle_mpv_command(stream, path, mpv);
    }
}

fn handle_mpv_command(stream: &mut TcpStream, path: String, mpv: &Mpv) {
    // to stop from RelativeUrlwithoutBase
    let url = url::Url::parse(&format!("rel:{}", path)).unwrap();
    let command = &url.path()[1..];
    match command {
        "peek" => {
            let title = mpv
                .get_property::<String>("media-title")
                .unwrap_or_default();
            let time = mpv.get_property::<f64>("time-pos").unwrap_or_default();
            let duration = mpv.get_property::<f64>("duration").unwrap_or_default();
            let percentage = mpv.get_property::<f64>("percent-pos").unwrap_or_default();
            serve_requested_text(&format!("{percentage} {time} {duration}\n{title}"), stream);
        }
        "playpause" => {
            _ = if let Ok(true) = mpv.get_property::<bool>("pause") {
                mpv.unpause()
            } else {
                mpv.pause()
            };
        }
        "pause" => {
            _ = mpv.pause();
        }
        "play" => {
            _ = mpv.unpause();
        }
        "next" => {
            if mpv.playlist_next_weak().is_ok() {
                serve_success(stream);
            }
        }
        "prev" => {
            if mpv.playlist_previous_weak().is_ok() {
                serve_success(stream);
            }
        }
        "append" => {
            if let Some(media) = url.query_pairs().filter(|(k, _)| k == "url").next() {
                if mpv
                    .playlist_load_files(&[(&media.1, FileState::AppendPlay, None)])
                    .is_ok()
                {
                    serve_success(stream);
                }
            }
        }
        "replace" => {
            if let Some(media) = url.query_pairs().filter(|(k, _)| k == "url").next() {
                if mpv
                    .playlist_load_files(&[(&media.1, FileState::Replace, None)])
                    .is_ok()
                {
                    serve_success(stream);
                }
            }
        }
        "seek" => {
            if let Some((par, val)) = url.query_pairs().last() {
                if let Ok(val) = val.parse() {
                    match par.as_ref() {
                        "forward" => {
                            _ = mpv.seek_forward(val);
                        }
                        "backward" => {
                            _ = mpv.seek_backward(val);
                        }
                        "percent" => {
                            let duration = mpv.get_property::<f64>("duration").unwrap_or_default();
                            _ = mpv.seek_absolute(val / 100.0 * duration);
                        }
                        _ => (),
                    }
                }
            }
        }
        "playlist" => {
            let playlist = mpv
                .get_property::<String>("playlist")
                .unwrap_or("[]".to_string());
            serve_requested_text(&playlist, stream);
        }
        "clear" => {
            if mpv.playlist_clear().is_ok() {
                serve_success(stream);
            }
        }
        "fullscreen" => {
            let fs = mpv.get_property::<bool>("fullscreen").unwrap_or(false);
            _ = mpv.set_property("fullscreen", !fs);
        }
        "mute" => {
            let fs = mpv.get_property::<bool>("mute").unwrap_or(false);
            _ = mpv.set_property("mute", !fs);
        }
        "stop" => {
            if mpv.playlist_clear().is_ok() {
                _ = mpv.playlist_next_force();
                serve_success(stream);
            }
        }
        _ => serve_bad_request(stream),
    };
}

fn serve_requested_file(file_path: &str, stream: &mut TcpStream) {
    // Construct the full file path, if "/" the use index.html
    let file_path = if file_path == "/" {
        "index.html"
    } else {
        &file_path[1..]
    };

    let path = Path::new(&file_path);

    // Generate the HTTP response
    let response = match fs::read_to_string(&path) {
        Ok(contents) => format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
            contents.len(),
            contents
        ),
        Err(_) => {
            let not_found = "404 Not Found.";
            format!(
                "HTTP/1.1 404 NOT FOUND\r\nContent-Length: {}\r\n\r\n{}",
                not_found.len(),
                not_found
            )
        }
    };

    // Send the response over the TCP stream
    stream.write(response.as_bytes()).unwrap();
    stream.flush().unwrap();
}

fn serve_success(stream: &mut TcpStream) {
    // Generate the HTTP response
    let contents = "SUCCESS";
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
        contents.len(),
        contents
    );
    // Send the response over the TCP stream
    stream.write(response.as_bytes()).unwrap();
    stream.flush().unwrap();
}

fn serve_bad_request(stream: &mut TcpStream) {
    // Generate the HTTP response
    let contents = "No Such End Point in API";
    let response = format!(
        "HTTP/1.1 400 Bad Request\r\nContent-Length: {}\r\n\r\n{}",
        contents.len(),
        contents
    );
    // Send the response over the TCP stream
    stream.write(response.as_bytes()).unwrap();
    stream.flush().unwrap();
}

fn serve_requested_text(contents: &str, stream: &mut TcpStream) {
    // Generate the HTTP response
    let response = format!(
        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n{}",
        contents.len(),
        contents
    );
    // Send the response over the TCP stream
    stream.write(response.as_bytes()).unwrap();
    stream.flush().unwrap();
}
