# unzip

### Cross compilation

```
rustup target add armeabi-v7a-linux-android
rustup target add arm64-v8a-linux-android
rustup target add x86_64-linux-android
cargo install cargo-ndk
```

```
export NDK_HOME=~/Android/android-ndk-r27
cargo ndk -t armeabi-v7a build --release
cargo ndk -t arm64-v8a build --release
cargo ndk -t x86_64 build --release
```