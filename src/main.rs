use clap::Parser;
use std::fs;
use std::io::prelude::*;
use std::net::{TcpListener, TcpStream};
use std::path::Path;
use std::process::exit;

use arboard::Clipboard;
use libmpv::{events::Event, FileState, Mpv};

#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Args {
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

    /// Any args to pass to mpv
    #[arg(num_args(0..), last(true))]
    args: Vec<String>,
}

fn main() -> libmpv::Result<()> {
    let args = Args::parse();
    let mut ctx = Clipboard::new().unwrap();
    let mut clip_txt = ctx.get_text().unwrap_or_else(|_| String::from(""));

    if args.server {
        let addr = if_addrs::get_if_addrs()
            .unwrap()
            .into_iter()
            .map(|a| a.ip())
            .filter(|a| a.is_ipv4() && !a.to_string().contains("127.0.0.1"))
            .next()
            .unwrap();
        let addr = format!("http://{addr}:{}", args.port);
        println!("{}", addr);
        fast_qr::QRBuilder::new(addr).build().unwrap().print();
    }

    let mpv = Mpv::new()?;
    mpv.set_property("idle", "yes")?;
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

    std::thread::scope(|s| {
        s.spawn(|| loop {
            let clip_new = ctx.get_text().unwrap_or_else(|_| String::from(""));

            if clip_new != clip_txt {
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
                clip_txt = clip_new;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        });

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
                let endpoint = format!("127.0.0.1:{}", args.port);
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

    println!("Request: {}", request_str);

    if request_str.starts_with("GET") {
        let filepath = request_str
            .split_whitespace()
            .nth(1)
            .unwrap_or("/")
            .to_string();
        serve_requested_file(&filepath, stream);
    } else if request_str.starts_with("POST") {
        handle_mpv_command(stream, &request_str, mpv);
    }
}

fn handle_mpv_command(stream: &mut TcpStream, request_str: &str, mpv: &Mpv) {
    // let response = "HTTP/1.1 200 OK\r\n\r\n";
    // stream.write(response.as_bytes()).unwrap();
    // stream.flush().unwrap();
    if request_str.contains("pause=true") {
        mpv.pause().unwrap();
    }
    if request_str.contains("unpause=true") {
        mpv.unpause().unwrap();
    }
    serve_requested_file("/", stream);
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
