# Source before Android builds: `source tools/android-env.sh`.
export ANDROID_HOME=$HOME/Library/Android/sdk
export NDK_HOME=$ANDROID_HOME/ndk/$(ls "$ANDROID_HOME/ndk" | sort -V | tail -1)
export JAVA_HOME=$(brew --prefix openjdk@17)/libexec/openjdk.jdk/Contents/Home
export PATH=$JAVA_HOME/bin:$ANDROID_HOME/platform-tools:$PATH
