set -x

if [ $TRAVIS_OS_NAME = linux ]; then
    # Trusty uses pretty old versions => use newer

    # GStreamer
    curl -L https://people.freedesktop.org/~slomo/gstreamer.tar.gz | tar xz
    sed -i "s;prefix=/root/gstreamer;prefix=$PWD/gstreamer;g" $PWD/gstreamer/lib/x86_64-linux-gnu/pkgconfig/*.pc
    export PKG_CONFIG_PATH=$PWD/gstreamer/lib/x86_64-linux-gnu/pkgconfig
    export LD_LIBRARY_PATH=$PWD/gstreamer/lib/x86_64-linux-gnu:$LD_LIBRARY_PATH
elif [ $TRAVIS_OS_NAME = osx ]; then
    brew update
    brew install gtk+3 gstreamer
else:
    echo Unknown OS $TRAVIS_OS_NAME
fi

set +x
