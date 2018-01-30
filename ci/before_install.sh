set -ex

main() {
    if [ $TRAVIS_OS_NAME = linux ]; then
        curl -L https://people.freedesktop.org/~slomo/gstreamer.tar.gz | tar xz
        sed -i "s;prefix=/root/gstreamer;prefix=$PWD/gstreamer;g" $PWD/gstreamer/lib/pkgconfig/*.pc
        export PKG_CONFIG_PATH=$PWD/gstreamer/lib/pkgconfig
        export GST_PLUGIN_SYSTEM_PATH=$PWD/gstreamer/lib/gstreamer-1.0
        export GST_PLUGIN_SCANNER=$PWD/gstreamer/libexec/gstreamer-1.0/gst-plugin-scanner
        export PATH=$PATH:$PWD/gstreamer/bin
        export LD_LIBRARY_PATH=$PWD/gstreamer/lib:$LD_LIBRARY_PATH
    else
        brew update
        brew install gtk+3 gstreamer
    fi
}

main
