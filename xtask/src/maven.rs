//! The Maven Central bundle of the JVM binding (phase 5.10, D-304): `cargo xtask package-jvm
//! --maven <dir> [--sign]`, from the JAR and the AAR a release built (`teigeodata-<v>.jar`,
//! `teigeodata-<v>.aar` in `<dir>`).
//!
//! Two components under the group `com.teispace` (D-211): `teigeodata` (the JAR, every desktop
//! platform's library inside) and `teigeodata-android` (the AAR). Each gets the files the Central
//! Portal requires: a POM with name, description, URL, licence, developer and SCM, a
//! `-sources.jar`, a `-javadoc.jar` (javadoc with doclint's groups as errors, but `missing`), and
//! an MD5 and a SHA-1 of every file; `--sign` adds a GPG signature (`.asc`) of each, with the key
//! `gpg` has. The tree is written to `target/jvm/maven/` and zipped as
//! `target/jvm/central-bundle.zip`, the Portal's upload format.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use md5::Md5;
use sha1::{Digest, Sha1};

use crate::package_c::command;

const GROUP: &str = "com.teispace";
const SOURCES: &str = "bindings/jvm/java";

/// One component: its artifact ID, packaging, the built file's extension and its description.
struct Component {
    artifact: &'static str,
    packaging: &'static str,
    description: &'static str,
}

const COMPONENTS: [Component; 2] = [
    Component {
        artifact: "teigeodata",
        packaging: "jar",
        description: "Accurate, offline places for the JVM: coordinates, elevation, the admin \
                      hierarchy, local names, time zones and postal codes from signed, compact \
                      packs. The native library for Linux, macOS and Windows is inside.",
    },
    Component {
        artifact: "teigeodata-android",
        packaging: "aar",
        description: "Accurate, offline places for Android: coordinates, elevation, the admin \
                      hierarchy, local names, time zones and postal codes from signed, compact \
                      packs.",
    },
];

pub(crate) fn bundle(
    root: &Path,
    out: &Path,
    version: &str,
    built: &Path,
    sign: bool,
) -> Result<String, Vec<String>> {
    let io = |p: &Path, e: std::io::Error| vec![format!("{}: {e}", p.display())];
    let tree = out.join("maven");
    let _ = std::fs::remove_dir_all(&tree);
    let scratch = out.join("maven-scratch");
    let _ = std::fs::remove_dir_all(&scratch);
    std::fs::create_dir_all(&scratch).map_err(|e| io(&scratch, e))?;

    let sources_jar = scratch.join("sources.jar");
    jar(root, &sources_jar, &root.join(SOURCES))?;
    let docs = scratch.join("javadoc");
    command(
        root,
        "javadoc",
        &[
            "--release",
            "17",
            "-quiet",
            // Every group but `missing`: the API documents its parameters in each method's
            // prose (the Rust convention it is generated from), not one `@param` at a time.
            "-Xdoclint:all,-missing",
            "-Werror",
            "-d",
            &docs.display().to_string(),
            "-sourcepath",
            &root.join(SOURCES).display().to_string(),
            "-subpackages",
            "org.teispace.teigeodata",
        ],
    )?;
    let javadoc_jar = scratch.join("javadoc.jar");
    jar(root, &javadoc_jar, &docs)?;

    let mut files = 0;
    for c in &COMPONENTS {
        let dir = tree
            .join(GROUP.replace('.', "/"))
            .join(c.artifact)
            .join(version);
        std::fs::create_dir_all(&dir).map_err(|e| io(&dir, e))?;
        let base = format!("{}-{version}", c.artifact);
        let main = built.join(format!("teigeodata-{version}.{}", c.packaging));
        let copies = [
            (main, format!("{base}.{}", c.packaging)),
            (sources_jar.clone(), format!("{base}-sources.jar")),
            (javadoc_jar.clone(), format!("{base}-javadoc.jar")),
        ];
        for (from, name) in &copies {
            std::fs::copy(from, dir.join(name)).map_err(|e| io(from, e))?;
        }
        let pom = dir.join(format!("{base}.pom"));
        std::fs::write(&pom, pom_xml(c, version)).map_err(|e| io(&pom, e))?;
        let mut names: Vec<String> = copies.into_iter().map(|(_, n)| n).collect();
        names.push(format!("{base}.pom"));
        for name in &names {
            let file = dir.join(name);
            let bytes = std::fs::read(&file).map_err(|e| io(&file, e))?;
            let md5 = dir.join(format!("{name}.md5"));
            std::fs::write(&md5, hex(&Md5::digest(&bytes))).map_err(|e| io(&md5, e))?;
            let sha1 = dir.join(format!("{name}.sha1"));
            std::fs::write(&sha1, hex(&Sha1::digest(&bytes))).map_err(|e| io(&sha1, e))?;
            if sign {
                command(
                    root,
                    "gpg",
                    &[
                        "--batch",
                        "--yes",
                        "--armor",
                        "--detach-sign",
                        &file.display().to_string(),
                    ],
                )?;
            }
            files += 1;
        }
    }
    let zip = out.join("central-bundle.zip");
    let _ = std::fs::remove_file(&zip);
    command(
        root,
        "jar",
        &[
            "--create",
            "--no-manifest",
            "--file",
            &zip.display().to_string(),
            "-C",
            &tree.display().to_string(),
            ".",
        ],
    )?;
    let _ = std::fs::remove_dir_all(&scratch);
    let kb = std::fs::metadata(&zip).map_or(0, |m| m.len()) / 1024;
    Ok(format!(
        "{}: {kb} KB ({} components, {files} files, each with MD5 and SHA-1{})",
        zip.display(),
        COMPONENTS.len(),
        if sign {
            " and a signature"
        } else {
            "; unsigned"
        }
    ))
}

fn jar(root: &Path, file: &Path, dir: &Path) -> Result<(), Vec<String>> {
    command(
        root,
        "jar",
        &[
            "--create",
            "--file",
            &file.display().to_string(),
            "-C",
            &dir.display().to_string(),
            ".",
        ],
    )
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

fn pom_xml(c: &Component, version: &str) -> String {
    let repo = "https://github.com/teispace/teigeodata";
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<project xmlns="http://maven.apache.org/POM/4.0.0"
         xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance"
         xsi:schemaLocation="http://maven.apache.org/POM/4.0.0 https://maven.apache.org/xsd/maven-4.0.0.xsd">
  <modelVersion>4.0.0</modelVersion>
  <groupId>{GROUP}</groupId>
  <artifactId>{artifact}</artifactId>
  <version>{version}</version>
  <packaging>{packaging}</packaging>
  <name>{artifact}</name>
  <description>{description}</description>
  <url>{repo}</url>
  <licenses>
    <license>
      <name>Apache-2.0</name>
      <url>https://www.apache.org/licenses/LICENSE-2.0.txt</url>
      <distribution>repo</distribution>
    </license>
  </licenses>
  <developers>
    <developer>
      <id>teispace</id>
      <name>Teispace</name>
      <organization>Teispace</organization>
      <organizationUrl>https://teispace.com</organizationUrl>
    </developer>
  </developers>
  <scm>
    <connection>scm:git:{repo}.git</connection>
    <developerConnection>scm:git:ssh://git@github.com/teispace/teigeodata.git</developerConnection>
    <url>{repo}</url>
    <tag>v{version}</tag>
  </scm>
</project>
"#,
        artifact = c.artifact,
        packaging = c.packaging,
        description = c.description,
    )
}

/// The folder a release's JAR and AAR are read from when none is named: where
/// `package-jvm --android --desktop` writes them.
pub(crate) fn default_dir(root: &Path) -> PathBuf {
    root.join("target/jvm")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_pom_has_what_the_central_portal_requires() {
        for c in &COMPONENTS {
            let pom = pom_xml(c, "1.2.3");
            for tag in [
                "<groupId>com.teispace</groupId>",
                "<version>1.2.3</version>",
                "<name>",
                "<description>",
                "<url>",
                "<licenses>",
                "<developers>",
                "<scm>",
                "<connection>",
                "<tag>v1.2.3</tag>",
            ] {
                assert!(pom.contains(tag), "{}: no {tag}", c.artifact);
            }
            assert!(pom.contains(&format!("<packaging>{}</packaging>", c.packaging)));
            assert!(
                !c.description.contains("  "),
                "{}: a wrapped description",
                c.artifact
            );
        }
    }

    #[test]
    fn checksums_are_lowercase_hex() {
        assert_eq!(hex(&Md5::digest(b"")), "d41d8cd98f00b204e9800998ecf8427e");
        assert_eq!(
            hex(&Sha1::digest(b"")),
            "da39a3ee5e6b4b0d3255bfef95601890afd80709"
        );
    }
}
