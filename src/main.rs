use clap::Parser;
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

    /// Any args to pass to mpv
    #[arg(num_args(0..), last(true))]
    args: Vec<String>,
}

fn main() -> libmpv::Result<()> {
    let args = Args::parse();

    let mut ctx = Clipboard::new().unwrap();
    let mut clip_txt = ctx.get_text().unwrap_or_else(|_| String::from(""));

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
    });
    Ok(())
}
