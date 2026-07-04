use crate::composer::RequiredPackage;

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub(crate) struct Lockfile {
    pub lockfile_version: u8,
    pub root_requirements_hash: String,
    pub root_requirements: Vec<RequiredPackage>,
    pub packages: Vec<LockedPackage>,
}

#[derive(serde::Serialize, serde::Deserialize, Debug)]
pub(crate) struct LockedPackage {
    pub name: String,
    pub version: String,
    pub dist_url: String,
    pub package_requires: Vec<RequiredPackage>,
    pub platform_requires: Vec<RequiredPackage>,
}

pub(crate) const LOCKFILE_VERSION: u8 = 1;
pub(crate) const LOCKFILE_PATH: &str = "concerto.lock";

pub(crate) fn write(lockfile: &Lockfile) -> Result<(), String> {
    let content = serde_json::to_string_pretty(lockfile)
        .map_err(|error| format!("Could not serialize lockfile: {error}"))?;

    std::fs::write(LOCKFILE_PATH, content)
        .map_err(|error| format!("Could not write lockfile: {error}"))
}

pub(crate) fn read() -> Result<Option<Lockfile>, String> {
    match std::fs::read_to_string(LOCKFILE_PATH) {
        Ok(content) => parse(&content).map(Some),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(format!("Could not read lockfile: {error}")),
    }
}

fn parse(content: &str) -> Result<Lockfile, String> {
    let lockfile: Lockfile =
        serde_json::from_str(content).map_err(|error| format!("Invalid lockfile: {error}"))?;

    if lockfile.lockfile_version != LOCKFILE_VERSION {
        return Err(format!(
            "Unsupported lockfile version: {}",
            lockfile.lockfile_version
        ));
    }

    if lockfile.root_requirements_hash != root_requirements_hash(&lockfile.root_requirements) {
        return Err("Lockfile root requirements hash does not match requirements".to_string());
    }

    Ok(lockfile)
}

pub(crate) fn matches_root_requirements(
    lockfile: &Lockfile,
    root_requirements: &[RequiredPackage],
) -> bool {
    lockfile.root_requirements_hash == root_requirements_hash(root_requirements)
}

pub(crate) fn root_requirements_hash(root_requirements: &[RequiredPackage]) -> String {
    let mut requirements = root_requirements.to_vec();
    requirements.sort_by(|left, right| left.name.cmp(&right.name));

    let mut hasher = blake3::Hasher::new();

    for requirement in requirements {
        hash_value(&mut hasher, &requirement.name);
        hash_value(&mut hasher, &requirement.constraint);
    }

    hasher.finalize().to_hex().to_string()
}

fn hash_value(hasher: &mut blake3::Hasher, value: &str) {
    hasher.update(value.len().to_string().as_bytes());
    hasher.update(b":");
    hasher.update(value.as_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lockfile() {
        let root_requirements = vec![required_package("psr/log", "^3.0")];
        let root_requirements_hash = root_requirements_hash(&root_requirements);
        let content = format!(
            r#"
        {{
          "lockfile_version": 1,
          "root_requirements_hash": "{root_requirements_hash}",
          "root_requirements": [
            {{
              "name": "psr/log",
              "constraint": "^3.0"
            }}
          ],
          "packages": [
            {{
              "name": "psr/log",
              "version": "3.0.2",
              "dist_url": "https://example.com/psr-log.zip",
              "package_requires": [],
              "platform_requires": []
            }}
          ]
        }}
        "#
        );

        let lockfile = parse(&content).unwrap();

        assert_eq!(lockfile.root_requirements_hash, root_requirements_hash);
        assert_eq!(lockfile.root_requirements.len(), 1);
        assert_eq!(lockfile.root_requirements[0].name, "psr/log");
        assert_eq!(lockfile.root_requirements[0].constraint, "^3.0");
        assert_eq!(lockfile.packages.len(), 1);
        assert_eq!(lockfile.packages[0].name, "psr/log");
        assert_eq!(lockfile.packages[0].version, "3.0.2");
    }

    #[test]
    fn matches_same_root_requirements() {
        let locked_root_requirements = vec![
            required_package("psr/log", "^3.0"),
            required_package("monolog/monolog", "^3.0"),
        ];
        let lockfile = Lockfile {
            lockfile_version: LOCKFILE_VERSION,
            root_requirements_hash: root_requirements_hash(&locked_root_requirements),
            root_requirements: locked_root_requirements,
            packages: Vec::new(),
        };

        let root_requirements = vec![
            required_package("monolog/monolog", "^3.0"),
            required_package("psr/log", "^3.0"),
        ];

        assert!(matches_root_requirements(&lockfile, &root_requirements));
    }

    #[test]
    fn rejects_changed_root_requirements() {
        let locked_root_requirements = vec![required_package("psr/log", "^3.0")];
        let lockfile = Lockfile {
            lockfile_version: LOCKFILE_VERSION,
            root_requirements_hash: root_requirements_hash(&locked_root_requirements),
            root_requirements: locked_root_requirements,
            packages: Vec::new(),
        };

        let root_requirements = vec![required_package("psr/log", "^2.0")];

        assert!(!matches_root_requirements(&lockfile, &root_requirements));
    }

    #[test]
    fn rejects_unsupported_lockfile_version() {
        let content = r#"
    {
      "lockfile_version": 2,
      "root_requirements_hash": "test-hash",
      "root_requirements": [],
      "packages": []
    }
    "#;

        let error = parse(content).unwrap_err();

        assert!(error.contains("Unsupported lockfile version: 2"));
    }

    #[test]
    fn rejects_mismatched_root_requirements_hash() {
        let content = r#"
    {
      "lockfile_version": 1,
      "root_requirements_hash": "wrong-hash",
      "root_requirements": [],
      "packages": []
    }
    "#;

        let error = parse(content).unwrap_err();

        assert!(error.contains("Lockfile root requirements hash does not match requirements"));
    }

    #[test]
    fn hashes_root_requirements_independently_from_order() {
        let left = vec![
            required_package("psr/log", "^3.0"),
            required_package("monolog/monolog", "^3.0"),
        ];

        let right = vec![
            required_package("monolog/monolog", "^3.0"),
            required_package("psr/log", "^3.0"),
        ];

        assert_eq!(
            root_requirements_hash(&left),
            root_requirements_hash(&right)
        );
    }

    #[test]
    fn hashes_changed_root_requirements_differently() {
        let original = vec![required_package("psr/log", "^3.0")];
        let changed = vec![required_package("psr/log", "^2.0")];

        assert_ne!(
            root_requirements_hash(&original),
            root_requirements_hash(&changed)
        );
    }

    fn required_package(name: &str, constraint: &str) -> RequiredPackage {
        RequiredPackage {
            name: name.to_string(),
            constraint: constraint.to_string(),
        }
    }
}
