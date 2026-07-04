# Lockfile format

Concerto writes `concerto.lock` as deterministic JSON. It is meant to be easy
to inspect, diff, and evolve without a custom parser.

## Current version

Concerto currently writes `lockfile_version: 1`.

When reading a lockfile, Concerto rejects unsupported versions instead of
guessing how to interpret them. This keeps future format changes explicit.

## Top-level fields

```json
{
  "lockfile_version": 1,
  "root_requirements_hash": "...",
  "root_requirements": [],
  "packages": []
}
```

| Field | Purpose |
| --- | --- |
| `lockfile_version` | Schema version supported by Concerto |
| `root_requirements_hash` | Hash used to detect stale root requirements |
| `root_requirements` | Sorted `composer.json` package requirements |
| `packages` | Sorted resolved package entries |

## Root requirements hash

`root_requirements_hash` is computed from the sorted root requirements.

For each root requirement, Concerto hashes:

- package name
- constraint

The hash is used as a cheap stale-lockfile check. If `composer.json` changes,
the current root requirements hash no longer matches the lockfile and Concerto
resolves dependencies again.

Concerto also validates that `root_requirements_hash` matches the
`root_requirements` stored in the lockfile. A lockfile with mismatched data is
rejected.

## Package entries

Each package entry stores the data needed to reinstall from the lockfile:

```json
{
  "name": "psr/log",
  "version": "3.0.2",
  "dist_url": "https://example.com/archive.zip",
  "package_requires": [],
  "platform_requires": []
}
```

| Field | Purpose |
| --- | --- |
| `name` | Composer package name |
| `version` | Resolved package version |
| `dist_url` | Archive URL used by the package store |
| `package_requires` | Package dependencies declared by the release |
| `platform_requires` | Platform requirements such as `php` or `ext-*` |

## Determinism

Concerto sorts root requirements and resolved packages by package name before
writing the lockfile. This keeps lockfile diffs stable when dependency
resolution returns the same result.

## Limits

Version 1 does not enforce platform requirements yet. It records them so a
future install step can validate `php`, `ext-*`, and `lib-*` constraints.
