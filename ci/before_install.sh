set -x

if [ $TRAVIS_OS_NAME = linux ]; then
    # Trusty uses pretty old versions => use newer

    # GStreamer
    curl -L https://people.freedesktop.org/~slomo/gstreamer.tar.gz | tar xz
    sed -i "s;prefix=/root/gstreamer;prefix=$PWD/gstreamer;g" $PWD/gstreamer/lib/pkgconfig/*.pc
    export PKG_CONFIG_PATH=$PWD/gstreamer/lib/pkgconfig
    export LD_LIBRARY_PATH=$PWD/gstreamer/lib:$LD_LIBRARY_PATH

    # GTK3
    WD="$PWD"
    cd $HOME
    curl -L https://github.com/gkoz/gtk-bootstrap/releases/download/gtk-3.18.1-2/deps.txz | tar xJ
    cd "$WD"
    export PKG_CONFIG_PATH="$HOME/local/lib/pkgconfig":$PKG_CONFIG_PATH
    export LD_LIBRARY_PATH="$HOME/local/lib/":$LD_LIBRARY_PATH
elif [ $TRAVIS_OS_NAME = osx ]; then
    brew update
    brew install gtk+3 gstreamer
else:
    echo Unknown OS $TRAVIS_OS_NAME
fi

set +x
