use super::*;

#[test]
fn parses_lockfile() {
    let root_requirements = vec![required_package("psr/log", "^3.0")];
    let root_repositories = vec![repository("https://repo.example.com")];
    let root_manifest_hash = root_manifest_hash(&root_requirements, &root_repositories);
    let content = format!(
        r#"
    {{
      "lockfile_version": 2,
      "root_manifest_hash": "{root_manifest_hash}",
      "root_requirements": [
        {{
          "name": "psr/log",
          "constraint": "^3.0"
        }}
      ],
      "root_repositories": [
        {{
          "url": "https://repo.example.com"
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

    assert_eq!(lockfile.root_manifest_hash, root_manifest_hash);
    assert_eq!(lockfile.root_requirements.len(), 1);
    assert_eq!(lockfile.root_requirements[0].name, "psr/log");
    assert_eq!(lockfile.root_requirements[0].constraint, "^3.0");
    assert_eq!(lockfile.root_repositories, root_repositories);
    assert_eq!(lockfile.packages.len(), 1);
    assert_eq!(lockfile.packages[0].name, "psr/log");
    assert_eq!(lockfile.packages[0].version, "3.0.2");
}

#[test]
fn matches_same_root_manifest() {
    let locked_root_requirements = vec![
        required_package("psr/log", "^3.0"),
        required_package("monolog/monolog", "^3.0"),
    ];
    let locked_root_repositories = vec![repository("https://repo.example.com")];
    let lockfile = Lockfile {
        lockfile_version: LOCKFILE_VERSION,
        root_manifest_hash: root_manifest_hash(
            &locked_root_requirements,
            &locked_root_repositories,
        ),
        root_requirements: locked_root_requirements,
        root_repositories: locked_root_repositories,
        packages: Vec::new(),
    };

    let root_requirements = vec![
        required_package("monolog/monolog", "^3.0"),
        required_package("psr/log", "^3.0"),
    ];

    assert!(matches_root_manifest(
        &lockfile,
        &root_requirements,
        &[repository("https://repo.example.com")]
    ));
}

#[test]
fn rejects_changed_root_manifest_requirements() {
    let locked_root_requirements = vec![required_package("psr/log", "^3.0")];
    let locked_root_repositories = Vec::new();
    let lockfile = Lockfile {
        lockfile_version: LOCKFILE_VERSION,
        root_manifest_hash: root_manifest_hash(
            &locked_root_requirements,
            &locked_root_repositories,
        ),
        root_requirements: locked_root_requirements,
        root_repositories: locked_root_repositories,
        packages: Vec::new(),
    };

    let root_requirements = vec![required_package("psr/log", "^2.0")];

    assert!(!matches_root_manifest(&lockfile, &root_requirements, &[]));
}

#[test]
fn rejects_changed_root_manifest_repositories() {
    let root_requirements = vec![required_package("psr/log", "^3.0")];
    let locked_root_repositories = vec![repository("https://repo.example.com")];
    let lockfile = Lockfile {
        lockfile_version: LOCKFILE_VERSION,
        root_manifest_hash: root_manifest_hash(&root_requirements, &locked_root_repositories),
        root_requirements: root_requirements.clone(),
        root_repositories: locked_root_repositories,
        packages: Vec::new(),
    };

    assert!(!matches_root_manifest(
        &lockfile,
        &root_requirements,
        &[repository("https://repo.changed.example.com")]
    ));
}

#[test]
fn rejects_unsupported_lockfile_version() {
    let content = r#"
{
  "lockfile_version": 1,
  "root_manifest_hash": "test-hash",
  "root_requirements": [],
  "root_repositories": [],
  "packages": []
}
"#;

    let error = parse(content).unwrap_err().to_string();

    assert!(error.contains("Unsupported lockfile version: 1"));
}

#[test]
fn rejects_mismatched_root_manifest_hash() {
    let content = r#"
{
  "lockfile_version": 2,
  "root_manifest_hash": "wrong-hash",
  "root_requirements": [],
  "root_repositories": [],
  "packages": []
}
"#;

    let error = parse(content).unwrap_err().to_string();

    assert!(error.contains("Lockfile root manifest hash does not match root requirements"));
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
        root_manifest_hash(&left, &[]),
        root_manifest_hash(&right, &[])
    );
}

#[test]
fn hashes_changed_root_requirements_differently() {
    let original = vec![required_package("psr/log", "^3.0")];
    let changed = vec![required_package("psr/log", "^2.0")];

    assert_ne!(
        root_manifest_hash(&original, &[]),
        root_manifest_hash(&changed, &[])
    );
}

#[test]
fn hashes_repository_order() {
    let requirements = vec![required_package("psr/log", "^3.0")];
    let left = vec![
        repository("https://first.example.com"),
        repository("https://second.example.com"),
    ];
    let right = vec![
        repository("https://second.example.com"),
        repository("https://first.example.com"),
    ];

    assert_ne!(
        root_manifest_hash(&requirements, &left),
        root_manifest_hash(&requirements, &right)
    );
}

fn required_package(name: &str, constraint: &str) -> RequiredPackage {
    RequiredPackage {
        name: name.to_string(),
        constraint: constraint.to_string(),
    }
}

fn repository(url: &str) -> ComposerRepository {
    ComposerRepository {
        url: url.to_string(),
    }
}
