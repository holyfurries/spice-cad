fn main() {
    let root = std::env::var("SMLIB_ROOT")
        .expect("Enter nix develop to build the native SMLib application");
    let output = std::env::var("OUT_DIR").expect("Cargo provides OUT_DIR");
    std::fs::write(
        format!("{output}/spice_smlib_types.h"),
        r#"
#include <SmApiBrep.h>
#include <SmApiGeneral.h>
#include <SmVector3d.h>
#include <SmFilletExecutive.h>
#include <cstddef>
#include <memory>
using SmFailureArray = SmTArray<SmTessellationFailure>;
using SmCurveArray = SmTArray<SmCurve*>;
inline std::unique_ptr<SmCurveArray> spice_curve_array() {
    return std::unique_ptr<SmCurveArray>(new (*SmApiGetOrCreateContext()) SmCurveArray());
}
inline long spice_fillet_edges(SmBrep* body, SmEdge* const* edges, size_t count, double radius) {
    SmTArray<SmEdge*> selected(count, const_cast<SmEdge**>(edges), count);
    SmFilletExecutive executive(*SmApiGetOrCreateContext(), body);
    auto status = executive.SetFilletParameters(body, selected, SM_FSG_CIRCULAR,
        SM_FS_CONST_RADIUS, radius, nullptr, 0.0, 0.0, 0.0, 0.0);
    if (status != SM_SUCCESS) { return status; }
    return executive.DoFillet();
}
static_assert(sizeof(SmVector3d) == 24);
static_assert(offsetof(SmVector3d, x) == 0);
static_assert(offsetof(SmVector3d, y) == 8);
static_assert(offsetof(SmVector3d, z) == 16);
"#,
    )
    .expect("Write generated ABI declarations");
    cxx_build::bridge("src/kernel.rs")
        .include(output)
        .include(format!("{root}/include"))
        .define("SM_GRAPHICS_CALLBACKS", "1")
        .define("NDEBUG", "1")
        .define("SM_USE_TBB", "1")
        .define("SM_API_EXPORTS", None)
        .std("c++17")
        .opt_level(3)
        .compile("spice_smlib_bridge");
    println!("cargo:rustc-link-search=native={root}/lib");
    println!("cargo:rustc-link-lib=static=sm_api");
    println!("cargo:rustc-link-lib=static=smlib");
    println!("cargo:rustc-link-lib=tbb");
    println!("cargo:rustc-link-lib=stdc++");
    println!("cargo:rerun-if-changed=src/kernel.rs");
    println!("cargo:rerun-if-env-changed=SMLIB_ROOT");
}
