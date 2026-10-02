# PCB stroke resources

These files are frozen from the audited Pomelo Web working-tree snapshot.
`manifest.json` records individual hashes and source identification. `core.json`
is a JSON conversion of the original core TypeScript glyph data; the numbered
pages preserve the original bytes.

The font resources are not relicensed under the application's MIT declaration.
Keep `LICENSE-KiCad-stroke.txt` with this resource set. Its upstream notices
identify GPL-2.0-or-later, MIT and SIL OFL 1.1 contributions. Final release
packaging and licensing review remain pending, as recorded in the manifest.

Reproduction: run `scripts/prepare-stroke-font-assets.py <Web root> assets/fonts/stroke`.
The generator verifies the audited core and notice hashes and audits all pages.
It does not establish equivalence to a fixed KiCad revision.
