.PHONY: build clean targets

TARGET = libunzip
LIBS = app/jni/libs

build:
	cargo build

clean:
	cargo clean
targets:
	cargo update

	mkdir -p $(LIBS)/arm64-v8a
	cargo ndk -t armeabi-v7a build --release
	cp target/aarch64-linux-android/release/$(TARGET).so $(LIBS)/arm64-v8a/

	mkdir -p $(LIBS)/armeabi-v7a
	cargo ndk -t arm64-v8a build --release
	cp target/armv7-linux-androideabi/release/$(TARGET).so $(LIBS)/armeabi-v7a/

	mkdir -p $(LIBS)/x86
	cargo ndk -t x86 build --release
	cp target/i686-linux-android/release/$(TARGET).so $(LIBS)/x86/