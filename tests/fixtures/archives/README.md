Archive tests generate their ZIP/TAR files in private temporary directories and
clean them up when each fixture is dropped. Generated archives are not tracked.

The legacy ZipCrypto compatibility fixture is embedded as bytes in
`tests/archive.rs`. It contains original synthetic `legacy.txt` data encrypted
with the password `fixture-password`. It was generated independently with
Info-ZIP (`zip -j -P`); running the tests requires no external archiver.
