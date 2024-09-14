use clap::Parser;
use std::fs;
use std::io::prelude::*;
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::process::exit;

use libmpv::{events::Event, FileState, Mpv};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
    /// run mpv in loop mode
    #[arg(short, long)]
    r#loop: bool,
    /// Do not show video play audio only
    #[arg(short, long)]
    no_video: bool,
    /// Fullscreen
    #[arg(short, long, conflicts_with = "no_video")]
    fullscreen: bool,
    /// Run the server in the given port
    #[arg(short, long, default_value = "6780")]
    port: u16,
    /// Display QR code for URL
    #[arg(short, long)]
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

    let addr: Vec<String> = if_addrs::get_if_addrs()
        .unwrap()
        .into_iter()
        .map(|a| a.ip().to_string())
        .collect();

    std::thread::scope(|s| {
        s.spawn(|| loop {
            let ev = ev_ctx.wait_event(600.).unwrap_or(Err(libmpv::Error::Null));
            match ev {
                Ok(Event::Shutdown) => {
                    exit(0)
                    // override shutdown to just clear the playlist and wait
                    // BUG: in i3 the window persists untill workspace is changed
                    // if mpv.playlist_clear().is_ok() {
                    //     _ = mpv.playlist_remove_current();
                    // }
                }
                _ => (),
            }
        });

        addr.iter()
            .map(|ip| ip.to_string())
            .filter(|url| !url.contains("//127.0.0.1:") && !url.contains("//[::1]:"))
            .for_each(|ip| {
                let hp_addr = format!("http://{ip}:{}", args.port);
                println!("{}", hp_addr);
                if args.qr {
                    fast_qr::QRBuilder::new(hp_addr)
                        .ecl(fast_qr::ECL::M)
                        .build()
                        .unwrap()
                        .print();
                }
            });
        for ip in &addr {
            s.spawn(|| {
                let endpoint = format!("{}:{}", ip.clone(), args.port);
                let listener = TcpListener::bind(endpoint).unwrap();
                for incoming_stream in listener.incoming() {
                    if let Ok(mut stream) = incoming_stream {
                        handle_connection(&mut stream, &mpv);
                    }
                }
            });
        }
    });
    Ok(())
}

fn handle_connection(stream: &mut TcpStream, mpv: &Mpv) {
    // Buffer to read the incoming request
    let mut buffer = [0; 1024];
    if let Err(e) = stream.read(&mut buffer) {
        eprintln!("Error reading stream: {e:?}");
        return;
    }

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
    let url = match url::Url::parse(&format!("rel:{}", path)) {
        Ok(u) => u,
        Err(e) => {
            eprintln!("Invalid Url: {path}\n{e:?}");
            return;
        }
    };
    let command = &url.path()[1..];
    match command {
        "peek" => {
            let title = mpv
                .get_property::<String>("media-title")
                .unwrap_or_default();
            let mute = mpv.get_property::<bool>("mute").unwrap_or_default();
            let volume = mpv.get_property::<f64>("volume").unwrap_or_default();
            let time = mpv.get_property::<f64>("time-pos").unwrap_or_default();
            let duration = mpv.get_property::<f64>("duration").unwrap_or_default();
            let percentage = mpv.get_property::<f64>("percent-pos").unwrap_or_default();
            serve_text(
                stream,
                &format!("{mute} {volume}\n{percentage} {time} {duration}\n{title}"),
                None,
            );
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
                serve_text(stream, "SUCCESS", None);
            }
        }
        "prev" => {
            if mpv.playlist_previous_weak().is_ok() {
                serve_text(stream, "SUCCESS", None);
            }
        }
        "select" => {
            if let Some((_, item)) = url.query_pairs().filter(|(k, _)| k == "item").next() {
                if let Ok(item) = item.parse::<i64>() {
                    if mpv.set_property("playlist-pos", item).is_ok() {
                        serve_text(stream, "SUCCESS", None);
                    }
                }
            }
        }
        "append" => {
            if let Some(media) = url.query_pairs().filter(|(k, _)| k == "url").next() {
                if mpv
                    .playlist_load_files(&[(&media.1, FileState::AppendPlay, None)])
                    .is_ok()
                {
                    serve_text(stream, "SUCCESS", None);
                }
            }
        }
        "replace" => {
            if let Some(media) = url.query_pairs().filter(|(k, _)| k == "url").next() {
                if mpv
                    .playlist_load_files(&[(&media.1, FileState::Replace, None)])
                    .is_ok()
                {
                    serve_text(stream, "SUCCESS", None);
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
        "remove" => {
            if let Some((var, val)) = url.query_pairs().last() {
                if var == "i" {
                    if let Ok(val) = val.parse() {
                        if mpv.playlist_remove_index(val).is_ok() {
                            serve_text(stream, "SUCCESS", None);
                        }
                    }
                }
            }
        }
        "playlist" => {
            let playlist = mpv
                .get_property::<String>("playlist")
                .unwrap_or("[]".to_string());
            serve_text(stream, &playlist, None);
        }
        "shuffle" => {
            if mpv.playlist_shuffle().is_ok() {
                serve_text(stream, "SUCCESS", None);
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
        "volume" => {
            if let Some((_, item)) = url.query_pairs().filter(|(k, _)| k == "value").next() {
                if let Ok(item) = item.parse::<f64>() {
                    if mpv.set_property("volume", item).is_ok() {
                        serve_text(stream, "SUCCESS", None);
                    }
                }
            }
        }
        "stop" => {
            if mpv.playlist_clear().is_ok() {
                _ = mpv.playlist_remove_current();
                serve_text(stream, "SUCCESS", None);
            }
        }
        "message" => {
            let msg = urlencoding::decode(url.query().unwrap_or_default());
            let msg = msg
                .as_ref()
                .map(|s| s.trim())
                .unwrap_or("*Invalid UTF-8 Message*");
            if mpv.set_property("osd-msg1", msg).is_ok() {
                serve_text(stream, "SUCCESS", None);
            }
        }
        _ => serve_text(stream, "No Such End Point in API", Some("400 Bad Request")),
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

    // I guess mime type is not required.... for now
    // let mime = mime_guess::from_path(path).first_or_text_plain();

    // Generate the HTTP response
    let (response, contents) = match fs::read(&path) {
        Ok(contents) => (
            format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n\r\n",
                contents.len()
            ),
            contents,
        ),
        Err(_) => {
            let not_found = "404 Not Found.";
            (
                format!(
                    "HTTP/1.1 404 NOT FOUND\r\nContent-Length: {}\r\n\r\n",
                    not_found.len(),
                ),
                not_found.bytes().collect(),
            )
        }
    };

    // Send the response over the TCP stream
    _ = stream.write(response.as_bytes());
    _ = stream.write(&contents);
    _ = stream.flush();
}

fn serve_text(stream: &mut TcpStream, text: &str, code: Option<&str>) {
    // Generate the HTTP response
    let response = format!(
        "HTTP/1.1 {}\r\nContent-Length: {}\r\n\r\n{}",
        code.unwrap_or("200 OK"),
        text.len(),
        text
    );
    // Send the response over the TCP stream
    _ = stream.write(response.as_bytes());
    _ = stream.flush();
}
