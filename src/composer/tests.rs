use super::*;

#[test]
fn reads_required_packages() {
    let composer_json = r#"
    {
        "require": {
            "monolog/monolog": "^3.0"
        }
    }
    "#;

    let packages = required_packages(composer_json).unwrap();

    assert_eq!(
        packages,
        vec![RequiredPackage {
            name: "monolog/monolog".to_string(),
            constraint: "^3.0".to_string(),
        }]
    );
}

#[test]
fn rejects_invalid_package_names() {
    assert!(package_path_parts("../evil").is_err());
    assert!(package_path_parts("monolog").is_err());
    assert!(package_path_parts("monolog/monolog").is_ok());
}

#[test]
fn distinguishes_package_names_from_platform_requirements() {
    assert!(is_package_name("psr/log"));
    assert!(is_package_name("monolog/monolog"));

    assert!(!is_package_name("php"));
    assert!(!is_package_name("ext-json"));
    assert!(!is_package_name("lib-icu"));
}
