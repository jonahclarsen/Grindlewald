// Generated Android projects and toolchains exist only on CI.
import { readFileSync, writeFileSync, cpSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
const root = 'src-tauri/gen/android';
const main = `${root}/app/src/main`;
const java = `${main}/java/com/jonahclarsen/grindlewald`;
for (const file of ['MobileBridge.kt', 'GrindlewaldUpdater.kt']) cpSync(`android/${file}`, `${java}/${file}`);
const activity = `${java}/MainActivity.kt`;
let text = readFileSync(activity, 'utf8');
text = text.replace('super.onCreate(savedInstanceState)', `super.onCreate(savedInstanceState)
        MobileBridge.attach(this)
        androidx.core.view.ViewCompat.setOnApplyWindowInsetsListener(findViewById(android.R.id.content)) { view, insets ->
            val bars = insets.getInsets(androidx.core.view.WindowInsetsCompat.Type.systemBars() or androidx.core.view.WindowInsetsCompat.Type.displayCutout() or androidx.core.view.WindowInsetsCompat.Type.ime())
            view.setPadding(bars.left, bars.top, bars.right, bars.bottom)
            insets
        }`);
writeFileSync(activity, text);
const manifest = `${main}/AndroidManifest.xml`;
text = readFileSync(manifest, 'utf8').replace('<application', `<uses-permission android:name="android.permission.BLUETOOTH" android:maxSdkVersion="30" />
    <uses-permission android:name="android.permission.BLUETOOTH_ADMIN" android:maxSdkVersion="30" />
    <uses-permission android:name="android.permission.ACCESS_FINE_LOCATION" android:maxSdkVersion="30" />
    <uses-permission android:name="android.permission.BLUETOOTH_SCAN" android:usesPermissionFlags="neverForLocation" />
    <uses-permission android:name="android.permission.BLUETOOTH_CONNECT" />
    <uses-permission android:name="android.permission.REQUEST_INSTALL_PACKAGES" />
    <uses-permission android:name="android.permission.UPDATE_PACKAGES_WITHOUT_USER_ACTION" />
    <uses-feature android:name="android.hardware.bluetooth_le" android:required="false" />
    <application`).replace('</application>', '<receiver android:name=".GrindlewaldUpdateReceiver" android:exported="false" /></application>');
writeFileSync(manifest, text);
const gradle = `${root}/app/build.gradle.kts`;
text = readFileSync(gradle, 'utf8');
const version = process.env.ANDROID_VERSION ?? '0.1.0';
const [major, minor, patch] = version.split('.').map(Number);
if (![major, minor, patch].every(Number.isInteger) || minor > 999 || patch > 999) throw Error('Invalid version');
text = text.replace(/versionCode\s*=.*/, `versionCode = ${major * 1000000 + minor * 1000 + patch}`)
  .replace(/versionName\s*=.*/, `versionName = "${version}"`);
if (process.env.ANDROID_KEYSTORE_PATH) {
  text = text.replace('buildTypes {', `signingConfigs {
        create("release") {
            storeFile = file(System.getenv("ANDROID_KEYSTORE_PATH"))
            storePassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
            keyAlias = "grindlewald"
            keyPassword = System.getenv("ANDROID_KEYSTORE_PASSWORD")
        }
    }
    buildTypes {`).replace('getByName("release") {', 'getByName("release") {\n            signingConfig = signingConfigs.getByName("release")');
}
writeFileSync(gradle, text);
// Use exactly the Java implementation belonging to the locked Rust BLE crate.
const registry = join(process.env.HOME, '.cargo/registry/src');
let ble;
for (const dir of readdirSync(registry)) {
  if (readdirSync(join(registry, dir)).includes('btleplug-0.13.5')) ble = join(registry, dir, 'btleplug-0.13.5/src/droidplug/java/src/main/java');
}
if (!ble) throw Error('Run cargo fetch before configuring Android');
cpSync(ble, `${main}/java`, {recursive:true});
writeFileSync(`${root}/app/proguard-rules.pro`, readFileSync(`${root}/app/proguard-rules.pro`, 'utf8') + '\n-keep class com.nonpolynomial.** { *; }\n-keep class io.github.gedgygedgy.** { *; }\n-keep class com.jonahclarsen.grindlewald.MobileBridge { *; }\n');
