# Maintainer: Gaurav Atreya <allmanpride@gmail.com>
pkgname=local-mpv
pkgver=0.2.0
pkgrel=1
pkgdesc="Tool to run mpv with a local server"
arch=('x86_64')
url="https://github.com/Atreyagaurav/${pkgname}"
license=('GPL3')
depends=('gcc-libs')
makedepends=('rust' 'cargo' 'git')

build() {
    cargo build --release
}

prepare() {
    # the libmpv-rs is not up to date for arch linux's libmpv
    cd "$srcdir"
    git apply local-arch.patch
    git clone https://github.com/ParadoxSpiral/libmpv-rs.git
    cp /usr/include/mpv/* libmpv-rs/libmpv-sys/include/
}

package() {
    cd "$srcdir"
    mkdir -p "$pkgdir/usr/bin"
    cp "../target/release/${pkgname}" "$pkgdir/usr/bin/${pkgname}"
    mkdir -p "$pkgdir/usr/share/applications"
    cp "../${pkgname}.desktop" "$pkgdir/usr/share/applications/${pkgname}.desktop"
}
