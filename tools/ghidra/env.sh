# Sourced by the other scripts: Homebrew's Ghidra and its keg-only JDK.
G=$(brew --prefix ghidra)/libexec
export JAVA_HOME=$(brew --prefix openjdk@25)/libexec/openjdk.jdk/Contents/Home
export PATH=$JAVA_HOME/bin:$PATH
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
