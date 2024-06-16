# Intro
This program uses `libmpv` and waits for changes in clipboard text. Once a new content is detected it'll pass that to the `libmpv` to play/append to playlist.

# Use Cases
This can be used as a simple temporary playlist maker. You can run it in `--append` mode and just copy a bunch of youtube URLs or any other urls you have your mpv set to play. Then the `wait-mpv` will play it one after another.

You can also use this for remote music play and such using clipboard sharing programs. You can use your phone to add urls into clipboard and share that on the machine running `wait-mpv`.

# Options

    Usage: wait-mpv [OPTIONS] [-- [ARGS]...]
    
    Arguments:
      [ARGS]...  Any args to pass to mpv
    
    Options:
      -l, --loop        run mpv in loop mode
      -a, --append      add things in playlist instead of playing it instantly
      -n, --no-video    Do not show video play audio only
      -f, --fullscreen  Fullscreen
      -h, --help        Print help
      -V, --version     Print version

# Not implemented
`ARGS` are not yet sent to `mpv` through `libmpv`.

# Future Plans
Maybe make it filter the copied contents for urls/filepaths only so that other copied text don't mess up the playlist.


