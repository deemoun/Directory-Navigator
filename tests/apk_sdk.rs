//! Explicit SDK validation against a private synthetic APK; no device access.
mod support;
use dn::{
    apk::{manifest::Changes, project::Project, tools::Tools, Package},
    task::Cancellation,
};
use std::{fs, io, path::PathBuf, process::Command};
#[test]
#[ignore = "requires Android SDK aapt2/android.jar and a Java runtime; creates a private test key"]
fn sdk_build_patch_align_sign_verify_and_wrong_password() -> io::Result<()> {
    let t = support::Fixture::new("apk-sdk")?.canonicalized()?;
    let tools = Tools::discover();
    assert!(
        tools.signing_available() && tools.keytool.is_some(),
        "Install SDK build-tools and Java"
    );
    let sdk = std::env::var_os("DN_APK_SDK_ROOT")
        .map(PathBuf::from)
        .or_else(|| {
            tools
                .adb
                .as_ref()
                .and_then(|p| p.parent())
                .and_then(|p| p.parent())
                .map(PathBuf::from)
        })
        .expect("Set DN_APK_SDK_ROOT");
    let mut platforms = fs::read_dir(sdk.join("platforms"))?
        .filter_map(Result::ok)
        .map(|e| e.path().join("android.jar"))
        .filter(|p| p.is_file())
        .collect::<Vec<_>>();
    platforms.sort();
    let framework = platforms.last().expect("Install an Android SDK platform");
    let aapt2 = tools
        .zipalign
        .as_ref()
        .unwrap()
        .with_file_name(format!("aapt2{}", std::env::consts::EXE_SUFFIX));
    let manifest = t.0.join("AndroidManifest.xml");
    fs::write(
        &manifest,
        r#"<manifest xmlns:android="http://schemas.android.com/apk/res/android" package="org.dn.fixture" android:versionCode="1" android:versionName="1"><uses-sdk android:minSdkVersion="23"/><application android:label="DN Fixture" android:hasCode="false"/></manifest>"#,
    )?;
    let input = t.0.join("input.apk");
    let status = Command::new(aapt2)
        .arg("link")
        .arg("--manifest")
        .arg(&manifest)
        .arg("-I")
        .arg(framework)
        .arg("-o")
        .arg(&input)
        .status()?;
    assert!(status.success());
    let c = Cancellation::default();
    let package = Package::open(input.clone(), &c)?;
    let original = fs::read(&input)?;
    let mut project = Project::create(&package, t.0.join("project"), &c)?;
    project.save_changes(
        Changes {
            debuggable: Some(true),
            version_name: Some("DN 界".into()),
            ..Default::default()
        },
        &c,
    )?;
    let key = t.0.join("test.p12");
    tools.create_test_key(&key, &c)?;
    let output = t.0.join("signed.apk");
    tools.sign(&project, &output, &key, "androiddebugkey", "android", &c)?;
    let result = Package::open(output.clone(), &c)?;
    assert!(result.manifest.debuggable);
    assert_eq!(result.manifest.version_name, "DN 界");
    assert_eq!(fs::read(&input)?, original);
    assert!(tools
        .sign(&project, &output, &key, "androiddebugkey", "android", &c)
        .is_err());
    let failed = t.0.join("wrong.apk");
    let error = tools
        .sign(
            &project,
            &failed,
            &key,
            "androiddebugkey",
            "PRIVATE_WRONG_PASSWORD",
            &c,
        )
        .unwrap_err();
    assert!(!error.to_string().contains("PRIVATE_WRONG_PASSWORD"));
    assert!(!failed.exists());
    let jks = t.0.join("separate-passwords.jks");
    let status = Command::new(tools.keytool.as_ref().unwrap())
        .args([
            "-genkeypair",
            "-noprompt",
            "-storetype",
            "JKS",
            "-alias",
            "fixture",
            "-keyalg",
            "RSA",
            "-keysize",
            "2048",
            "-validity",
            "2",
            "-dname",
            "CN=DN SDK Test",
            "-storepass:env",
            "DN_TEST_STORE",
            "-keypass:env",
            "DN_TEST_KEY",
            "-keystore",
        ])
        .arg(&jks)
        .env("DN_TEST_STORE", "store-fixture-password")
        .env("DN_TEST_KEY", "key-fixture-password")
        .status()?;
    assert!(status.success());
    tools.sign_with_key(
        &project,
        &t.0.join("separate.apk"),
        &dn::apk::tools::SigningKey {
            path: &jks,
            alias: "fixture",
            password: "store-fixture-password",
            key_password: Some("key-fixture-password"),
        },
        &c,
    )?;
    assert!(!fs::read_dir(&t.0)?
        .filter_map(Result::ok)
        .any(|e| e.file_name().to_string_lossy().starts_with(".dn-apk-")));
    Ok(())
}
