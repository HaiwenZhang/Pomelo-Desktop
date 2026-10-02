fn main() {
    // The procedural macro reads a shared directory outside this crate.
    // Track it explicitly so translation-only edits invalidate compiled catalogs.
    println!("cargo:rerun-if-changed=../../locales");
}
