use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

fn main() -> io::Result<()> {
    let root = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let mut assets = Vec::new();
    collect(&root.join("skills"), &root, &mut assets)?;
    let templates = root.join("templates/runtime-repository");
    collect(&templates, &templates, &mut assets)?;
    assets.sort_by(|left, right| left.0.cmp(&right.0));

    println!("cargo:rerun-if-changed=skills");
    println!("cargo:rerun-if-changed=templates/runtime-repository");
    let entries = assets
        .iter()
        .map(|(target, source)| format!("({target:?}, include_bytes!({source:?}) as &[u8]),"))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("runtime_assets.rs"),
        format!("pub(crate) const ASSETS: &[(&str, &[u8])] = &[\n{entries}\n];\n"),
    )
}

fn collect(directory: &Path, root: &Path, assets: &mut Vec<(String, PathBuf)>) -> io::Result<()> {
    for entry in fs::read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        if entry.file_type()?.is_dir() {
            collect(&path, root, assets)?;
        } else if entry.file_type()?.is_file() {
            assets.push((
                path.strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .into_owned(),
                path,
            ));
        }
    }
    Ok(())
}
