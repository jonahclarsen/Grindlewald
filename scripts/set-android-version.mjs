import {readFileSync, writeFileSync} from 'node:fs';
const path = 'src-tauri/gen/android/app/build.gradle.kts';
const version = process.env.ANDROID_VERSION;
const [major, minor, patch] = version.split('.').map(Number);
writeFileSync(path, readFileSync(path, 'utf8').replace(/versionCode\s*=.*/, `versionCode = ${major*1000000+minor*1000+patch}`).replace(/versionName\s*=.*/, `versionName = "${version}"`));
