fn main() {
    let root = std::env::var("DEP_OCCT_ROOT")
        .expect("Enter nix develop .#verification to build the independent verifier");
    cxx_build::bridge("src/geometry.rs")
        .include(format!("{root}/include/opencascade"))
        .std("c++17")
        .opt_level(1)
        .compile("spice-occt-check");
    println!("cargo:rerun-if-env-changed=DEP_OCCT_ROOT");
    println!("cargo:rerun-if-changed=src/geometry.rs");
}
