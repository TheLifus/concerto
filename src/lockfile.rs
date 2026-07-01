use crate::composer::RequiredPackage;

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct Lockfile {
    pub root_requirements: Vec<RequiredPackage>,
    pub packages: Vec<LockedPackage>,
}

#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct LockedPackage {
    pub name: String,
    pub version: String,
    pub dist_url: String,
    pub package_requires: Vec<RequiredPackage>,
    pub platform_requires: Vec<RequiredPackage>,
}

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
    serde_json::from_str(content).map_err(|error| format!("Invalid lockfile: {error}"))
}

pub(crate) fn matches_root_requirements(
    lockfile: &Lockfile,
    root_requirements: &[RequiredPackage],
) -> bool {
    let mut locked = lockfile.root_requirements.clone();
    let mut current = root_requirements.to_vec();

    locked.sort_by(|left, right| left.name.cmp(&right.name));
    current.sort_by(|left, right| left.name.cmp(&right.name));

    locked == current
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_lockfile() {
        let content = r#"
        {
          "root_requirements": [
            {
              "name": "psr/log",
              "constraint": "^3.0"
            }
          ],
          "packages": [
            {
              "name": "psr/log",
              "version": "3.0.2",
              "dist_url": "https://example.com/psr-log.zip",
              "package_requires": [],
              "platform_requires": []
            }
          ]
        }
        "#;

        let lockfile = parse(content).unwrap();

        assert_eq!(lockfile.root_requirements.len(), 1);
        assert_eq!(lockfile.root_requirements[0].name, "psr/log");
        assert_eq!(lockfile.root_requirements[0].constraint, "^3.0");
        assert_eq!(lockfile.packages.len(), 1);
        assert_eq!(lockfile.packages[0].name, "psr/log");
        assert_eq!(lockfile.packages[0].version, "3.0.2");
    }

    #[test]
    fn matches_same_root_requirements() {
        let lockfile = Lockfile {
            root_requirements: vec![
                RequiredPackage {
                    name: "psr/log".to_string(),
                    constraint: "^3.0".to_string(),
                },
                RequiredPackage {
                    name: "monolog/monolog".to_string(),
                    constraint: "^3.0".to_string(),
                },
            ],
            packages: Vec::new(),
        };

        let root_requirements = vec![
            RequiredPackage {
                name: "monolog/monolog".to_string(),
                constraint: "^3.0".to_string(),
            },
            RequiredPackage {
                name: "psr/log".to_string(),
                constraint: "^3.0".to_string(),
            },
        ];

        assert!(matches_root_requirements(&lockfile, &root_requirements));
    }

    #[test]
    fn rejects_changed_root_requirements() {
        let lockfile = Lockfile {
            root_requirements: vec![RequiredPackage {
                name: "psr/log".to_string(),
                constraint: "^3.0".to_string(),
            }],
            packages: Vec::new(),
        };

        let root_requirements = vec![RequiredPackage {
            name: "psr/log".to_string(),
            constraint: "^2.0".to_string(),
        }];

        assert!(!matches_root_requirements(&lockfile, &root_requirements));
    }
}
