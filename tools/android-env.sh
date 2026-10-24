# Source before Android builds: `source tools/android-env.sh`.
export ANDROID_HOME=$HOME/Library/Android/sdk
export NDK_HOME=$ANDROID_HOME/ndk/$(ls "$ANDROID_HOME/ndk" | sort -V | tail -1)
export JAVA_HOME=$(brew --prefix openjdk@17)/libexec/openjdk.jdk/Contents/Home
export PATH=$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$PATH
# Plain `cargo build --target aarch64-linux-android` needs these; `cargo tauri android` sets its own.
ndk_bin=$NDK_HOME/toolchains/llvm/prebuilt/darwin-x86_64/bin
export CC_aarch64_linux_android=$ndk_bin/aarch64-linux-android26-clang
export AR_aarch64_linux_android=$ndk_bin/llvm-ar
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER=$ndk_bin/aarch64-linux-android26-clang
