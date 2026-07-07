# Lockfile format

Concerto writes `concerto.lock` as deterministic JSON. It is meant to be easy
to inspect, diff, and evolve without a custom parser.

## Current version

Concerto currently writes `lockfile_version: 2`.

When reading a lockfile, Concerto rejects unsupported versions instead of
guessing how to interpret them. This keeps future format changes explicit.

## Top-level fields

```json
{
  "lockfile_version": 2,
  "root_manifest_hash": "...",
  "root_requirements": [],
  "root_repositories": [],
  "packages": []
}
```

| Field | Purpose |
| --- | --- |
| `lockfile_version` | Schema version supported by Concerto |
| `root_manifest_hash` | Hash used to detect stale root requirements or repositories |
| `root_requirements` | Sorted `composer.json` package requirements |
| `root_repositories` | Root Composer repositories in priority order |
| `packages` | Sorted resolved package entries |

## Root manifest hash

`root_manifest_hash` is computed from the sorted root requirements and root
Composer repositories.

For each root requirement, Concerto hashes:

- package name
- constraint

For each root repository, Concerto hashes:

- repository URL

Repository order is preserved because Composer repository priority is ordered.

The hash is used as a cheap stale-lockfile check. If `composer.json` changes,
the current root manifest hash no longer matches the lockfile and Concerto
resolves dependencies again.

Concerto also validates that `root_manifest_hash` matches the
`root_requirements` and `root_repositories` stored in the lockfile. A lockfile
with mismatched data is rejected.

## Package entries

Each package entry stores the data needed to reinstall from the lockfile:

```json
{
  "name": "psr/log",
  "version": "3.0.2",
  "dist_url": "https://example.com/archive.zip",
  "dist_integrity": "blake3:...",
  "dist_shasum": "d1b237d28598c3eecb03447d38b3bc30b4baac44",
  "dev": false,
  "package_requires": [],
  "platform_requires": []
}
```

| Field | Purpose |
| --- | --- |
| `name` | Composer package name |
| `version` | Resolved package version |
| `dist_url` | Archive URL used by the package store |
| `dist_integrity` | BLAKE3 archive hash verified before extraction or store reuse |
| `dist_shasum` | Optional Packagist SHA-1 `dist.shasum`, verified when present |
| `dev` | Whether the package belongs only to the `require-dev` graph |
| `package_requires` | Package dependencies declared by the release |
| `platform_requires` | Platform requirements such as `php` or `ext-*` |

## Determinism

Concerto sorts root requirements and resolved packages by package name before
writing the lockfile. This keeps lockfile diffs stable when dependency
resolution returns the same result.

## Platform requirements

Version 2 stores platform requirements on each locked package.

Concerto validates supported platform requirements before installing from a
lockfile:

- `php` constraints are checked against the detected PHP version
- `ext-*` requirements are checked against loaded PHP extensions
- `lib-*` requirements are recognized but rejected as unsupported
