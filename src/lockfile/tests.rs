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

    let error = parse(content).unwrap_err().to_string();

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

    let error = parse(content).unwrap_err().to_string();

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
