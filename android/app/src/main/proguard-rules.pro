# Fuss release rules.
#
# R8 keeps the app small. The JNI layer is the one place where names must
# survive: Kotlin calls native methods by their Java name, so the class that
# declares them and the methods themselves are kept verbatim.

-keep class cloud.fuss.core.FussNative { *; }
-keep class cloud.fuss.core.FussException { *; }
-keepclasseswithmembernames class * {
    native <methods>;
}

# Compose and AndroidX ship their own rules; nothing extra is needed.

# Keep line numbers for useful crash reports, drop local variable tables.
-keepattributes SourceFile,LineNumberTable
-renamesourcefileattribute SourceFile
