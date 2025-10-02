# Maintainer: Tomasz Wyderka <twyderka@me.com>
pkgname=otterwatch
pkgver=0.0.5
pkgrel=1
makedepends=('rust' 'cargo')
arch=('i686' 'x86_64' 'armv6h' 'armv7h')
pkgdesc="Application for monitoring the performance of the Linux operating system"
url="https://github.com/ximot/OtterWatch"
license=('MIT')

build() {
    return 0
}

package() {
    cd $srcdir
    cargo install --root="$pkgdir" --git=https://github.com/ximot/OtterWatch --no-track
}
