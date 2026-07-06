## Fixtures

These fixtures let Concerto run install tests without Packagist or GitHub.
Tests point Concerto at generated metadata with `CONCERTO_PACKAGIST_FIXTURES_DIR`.

- `packages/` contains the readable package sources.
- `archives/` contains the ZIP archives used by `dist.url`.
- `packagist/` contains Packagist-like metadata templates.

To rebuild an archive after editing a package fixture:

```bash
cd tests/fixtures/packages
zip -Xqr ../archives/psr-log-3.0.2.zip psr-log-3.0.2
zip -Xqr ../archives/monolog-monolog-3.0.0.zip monolog-monolog-3.0.0
```
