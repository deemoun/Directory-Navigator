//! Synthetic Android fixtures. No APK binary is stored in the repository.
use std::{
    fs::File,
    io::{self, Write},
    path::Path,
};
fn put16(b: &mut [u8], p: usize, v: u16) {
    b[p..p + 2].copy_from_slice(&v.to_le_bytes());
}
fn put32(b: &mut [u8], p: usize, v: u32) {
    b[p..p + 4].copy_from_slice(&v.to_le_bytes());
}
struct Xml {
    strings: Vec<String>,
    chunks: Vec<Vec<u8>>,
}
impl Xml {
    fn intern(&mut self, s: &str) -> u32 {
        if let Some(i) = self.strings.iter().position(|v| v == s) {
            i as u32
        } else {
            self.strings.push(s.into());
            self.strings.len() as u32 - 1
        }
    }
    fn attr(&mut self, android: bool, name: &str, value: &str) -> Vec<u8> {
        let mut a = vec![0; 20];
        put32(
            &mut a,
            0,
            if android {
                self.intern("http://schemas.android.com/apk/res/android")
            } else {
                u32::MAX
            },
        );
        put32(&mut a, 4, self.intern(name));
        let i = self.intern(value);
        put32(&mut a, 8, i);
        put16(&mut a, 12, 8);
        a[15] = 3;
        put32(&mut a, 16, i);
        a
    }
    fn start(&mut self, name: &str, attrs: &[(bool, &str, &str)]) {
        let name = self.intern(name);
        let attrs = attrs
            .iter()
            .map(|(a, n, v)| self.attr(*a, n, v))
            .collect::<Vec<_>>();
        let mut c = vec![0; 36];
        put16(&mut c, 0, 0x102);
        put16(&mut c, 2, 16);
        put32(&mut c, 4, (36 + attrs.len() * 20) as u32);
        put32(&mut c, 12, u32::MAX);
        put32(&mut c, 16, u32::MAX);
        put32(&mut c, 20, name);
        put16(&mut c, 24, 20);
        put16(&mut c, 26, 20);
        put16(&mut c, 28, attrs.len() as u16);
        for a in attrs {
            c.extend(a);
        }
        self.chunks.push(c);
    }
    fn end(&mut self, name: &str) {
        let name = self.intern(name);
        let mut c = vec![0; 24];
        put16(&mut c, 0, 0x103);
        put16(&mut c, 2, 16);
        put32(&mut c, 4, 24);
        put32(&mut c, 12, u32::MAX);
        put32(&mut c, 16, u32::MAX);
        put32(&mut c, 20, name);
        self.chunks.push(c);
    }
    fn finish(self) -> Vec<u8> {
        let mut offsets = Vec::new();
        let mut text = Vec::new();
        for s in &self.strings {
            offsets.push(text.len() as u32);
            let u = s.encode_utf16().collect::<Vec<_>>();
            text.extend((u.len() as u16).to_le_bytes());
            for v in u {
                text.extend(v.to_le_bytes());
            }
            text.extend([0, 0]);
        }
        while text.len() % 4 != 0 {
            text.push(0);
        }
        let mut pool = vec![0; 28];
        put16(&mut pool, 0, 1);
        put16(&mut pool, 2, 28);
        put32(&mut pool, 4, (28 + offsets.len() * 4 + text.len()) as u32);
        put32(&mut pool, 8, offsets.len() as u32);
        put32(&mut pool, 20, (28 + offsets.len() * 4) as u32);
        for o in offsets {
            pool.extend(o.to_le_bytes());
        }
        pool.extend(text);
        let mut output = vec![3, 0, 8, 0, 0, 0, 0, 0];
        output.extend(pool);
        for c in self.chunks {
            output.extend(c);
        }
        let len = output.len() as u32;
        put32(&mut output, 4, len);
        output
    }
}
pub fn manifest() -> Vec<u8> {
    manifest_with_split(false)
}
pub fn manifest_with_split(split: bool) -> Vec<u8> {
    let mut x = Xml {
        strings: vec![],
        chunks: vec![],
    };
    let mut attrs = vec![
        (false, "package", "org.dn.fixture"),
        (true, "versionCode", "1"),
        (true, "versionName", "Тест 界"),
    ];
    if split {
        attrs.push((false, "split", "config.en"));
    }
    x.start("manifest", &attrs);
    x.start(
        "uses-sdk",
        &[
            (true, "minSdkVersion", "23"),
            (true, "targetSdkVersion", "35"),
        ],
    );
    x.end("uses-sdk");
    x.start(
        "uses-permission",
        &[
            (true, "name", "android.permission.INTERNET"),
            (true, "maxSdkVersion", "35"),
        ],
    );
    x.end("uses-permission");
    x.start("application", &[]);
    x.start("activity", &[(true, "name", ".MainActivity")]);
    x.start("intent-filter", &[]);
    x.start("action", &[(true, "name", "android.intent.action.MAIN")]);
    x.end("action");
    x.start(
        "category",
        &[(true, "name", "android.intent.category.LAUNCHER")],
    );
    x.end("category");
    x.end("intent-filter");
    x.end("activity");
    x.end("application");
    x.end("manifest");
    x.finish()
}
pub fn dex() -> Vec<u8> {
    let strings = [
        "Lorg/dn/fixture/Main;",
        "V",
        "run",
        "https://example.test/界",
    ];
    let mut b = vec![0; 224];
    b[..8].copy_from_slice(b"dex\n039\0");
    put32(&mut b, 36, 112);
    put32(&mut b, 40, 0x12345678);
    put32(&mut b, 56, 4);
    put32(&mut b, 60, 112);
    put32(&mut b, 64, 2);
    put32(&mut b, 68, 128);
    put32(&mut b, 72, 1);
    put32(&mut b, 76, 136);
    put32(&mut b, 88, 1);
    put32(&mut b, 92, 148);
    put32(&mut b, 96, 1);
    put32(&mut b, 100, 156);
    put32(&mut b, 128, 0);
    put32(&mut b, 132, 1);
    put32(&mut b, 136, 1);
    put32(&mut b, 140, 1);
    put32(&mut b, 152, 2);
    put32(&mut b, 180, 188);
    // One direct method; code starts at 200 (ULEB128 c8 01).
    b[188..195].copy_from_slice(&[0, 0, 1, 0, 0, 9, 0xc8]);
    b[195] = 1;
    put16(&mut b, 200, 1);
    put32(&mut b, 212, 3);
    put16(&mut b, 216, 0x001a);
    put16(&mut b, 218, 3);
    put16(&mut b, 220, 0x000e);
    for (i, s) in strings.iter().enumerate() {
        let off = b.len() as u32;
        put32(&mut b, 112 + i * 4, off);
        b.push(s.encode_utf16().count() as u8);
        b.extend(s.as_bytes());
        b.push(0);
    }
    let len = b.len() as u32;
    put32(&mut b, 32, len);
    b
}
pub fn write(path: &Path) -> io::Result<()> {
    let mut z = zip::ZipWriter::new(File::create(path)?);
    for (name, data) in [
        ("AndroidManifest.xml", manifest()),
        ("classes.dex", dex()),
        ("assets/config.txt", b"original config\n".to_vec()),
        ("resources.arsc", resources()),
        ("META-INF/CERT.SF", b"old signature".to_vec()),
        (
            "META-INF/services/example",
            b"preserve service metadata".to_vec(),
        ),
    ] {
        z.start_file(name, zip::write::SimpleFileOptions::default())
            .map_err(io::Error::other)?;
        z.write_all(&data)?;
    }
    z.add_directory("empty/", zip::write::SimpleFileOptions::default())
        .map_err(io::Error::other)?;
    z.finish().map_err(io::Error::other)?;
    Ok(())
}

pub fn resources() -> Vec<u8> {
    fn pool(values: &[&str]) -> Vec<u8> {
        Xml {
            strings: values.iter().map(|s| (*s).into()).collect(),
            chunks: vec![],
        }
        .finish()[8..]
            .to_vec()
    }
    let global = pool(&["DN Fixture 界"]);
    let types = pool(&["string"]);
    let keys = pool(&["app_name"]);
    let mut package = vec![0; 288];
    put16(&mut package, 0, 0x200);
    put16(&mut package, 2, 288);
    put32(&mut package, 8, 0x7f);
    put32(&mut package, 268, 288);
    put32(&mut package, 276, (288 + types.len()) as u32);
    package.extend(types);
    package.extend(keys);
    let mut ty = vec![0; 104];
    put16(&mut ty, 0, 0x201);
    put16(&mut ty, 2, 84);
    put32(&mut ty, 4, 104);
    ty[8] = 1;
    put32(&mut ty, 12, 1);
    put32(&mut ty, 16, 88);
    put32(&mut ty, 20, 64);
    put16(&mut ty, 88, 8);
    put16(&mut ty, 96, 8);
    ty[99] = 3;
    package.extend(ty);
    let size = package.len() as u32;
    put32(&mut package, 4, size);
    let mut data = vec![0; 12];
    put16(&mut data, 0, 2);
    put16(&mut data, 2, 12);
    put32(&mut data, 8, 1);
    data.extend(global);
    data.extend(package);
    let size = data.len() as u32;
    put32(&mut data, 4, size);
    data
}
