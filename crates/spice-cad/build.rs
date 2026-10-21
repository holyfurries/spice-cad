fn main() {
    let root = std::env::var("SMLIB_ROOT")
        .expect("Enter nix develop to build the native SMLib application");
    let output = std::env::var("OUT_DIR").expect("Cargo provides OUT_DIR");
    std::fs::write(
        format!("{output}/spice_smlib_types.h"),
        r#"
#include <SmApiBrep.h>
#include <SmApiGeneral.h>
#include <SmApiPrimitives.h>
#include <SmVector3d.h>
#include <SmFilletExecutive.h>
#include <SmApiCurves.h>
#include <SmBSplineCurve.h>
#include <SmAxis2Placement.h>
#include <cstddef>
#include <memory>
using SmFailureArray = SmTArray<SmTessellationFailure>;
using SmCurveArray = SmTArray<SmCurve*>;
inline long spice_loft(const SmCurveArray& curves, size_t section_count, size_t edge_count, unsigned int degree, SmBrep*& result) {
    if (section_count < 2 || section_count > 16 || edge_count < 2 || edge_count > 256 || curves.GetSize() != section_count * edge_count || degree < 1 || degree > 3 || degree >= section_count) { return SM_ERR_INVALID_INPUT; }
    SmTArray<ULONG> counts;
    SmTArray<const SmCurve*> sections;
    for (size_t index = 0; index < section_count; ++index) { counts.Add(edge_count); }
    for (size_t index = 0; index < curves.GetSize(); ++index) { sections.Add(curves[index]); }
    return SmApiCreateSkinPrimitive(counts, sections, degree, TRUE, result, nullptr, nullptr, nullptr);
}
inline long spice_shell(const SmBrep* body, double thickness, const SmFace* opening, SmBrep*& result) {
    SmTArray<const SmFace*> openings;
    if (opening) { openings.Add(opening); }
    return SmApiShellBrepFull(body, -thickness, TRUE, TRUE, FALSE, openings, result);
}
inline std::unique_ptr<SmCurve> spice_profile_arc(const SmVector3d& center, double radius, double start_deg, double end_deg) {
    const bool reverse = end_deg < start_deg;
    if (reverse) { std::swap(start_deg, end_deg); }
    if (end_deg > 360.0) { start_deg -= 360.0; end_deg -= 360.0; }
    if (start_deg < -360.0) { start_deg += 360.0; end_deg += 360.0; }
    SmBSplineCurve* curve = nullptr;
    auto status = SmApiCreateArc(center, radius, start_deg, end_deg, curve);
    if (status != SM_SUCCESS) { delete curve; return {}; }
    if (reverse) {
        SmExtent1d interval;
        status = curve->ReverseParameterization(curve->GetNaturalInterval(), interval);
        if (status != SM_SUCCESS) { delete curve; return {}; }
    }
    return std::unique_ptr<SmCurve>(curve);
}
inline std::unique_ptr<SmCurve> spice_path_arc(const SmVector3d& center, const SmVector3d& x_axis, const SmVector3d& y_axis, double radius, double angle_deg) {
    auto curve = spice_profile_arc(SmVector3d(0, 0, 0), radius, 0, angle_deg);
    if (!curve || curve->Transform(SmAxis2Placement(center, x_axis, y_axis)) != SM_SUCCESS) { return {}; }
    return curve;
}
inline std::unique_ptr<SmCurveArray> spice_curve_array() {
    return std::unique_ptr<SmCurveArray>(new (*SmApiGetOrCreateContext()) SmCurveArray());
}
inline std::unique_ptr<SmBrep> spice_copy_brep(const SmBrep& body) {
    return std::unique_ptr<SmBrep>(new (*SmApiGetOrCreateContext()) SmBrep(body, nullptr, nullptr, TRUE, FALSE));
}
inline long spice_treat_edges(SmBrep* body, SmEdge* const* edges, size_t count, double size_mm, bool chamfer) {
    SmTArray<SmEdge*> selected(count, const_cast<SmEdge**>(edges), count);
    SmFilletExecutive executive(*SmApiGetOrCreateContext(), body);
    auto status = executive.SetFilletParameters(body, selected,
        chamfer ? SM_FSG_LINEAR : SM_FSG_CIRCULAR,
        chamfer ? SM_FS_CONST_DIST : SM_FS_CONST_RADIUS, size_mm, nullptr, 0.0, 0.0, 0.0, 0.0);
    if (status != SM_SUCCESS) { return status; }
    if (chamfer) {
        status = executive.CreateFilletCorners();
        if (status != SM_SUCCESS) { return status; }
        auto& corners = executive.GetFilletCorners();
        if (corners.GetSize() > count * 2) { return SM_ERR_INVALID_INPUT; }
        for (size_t index = 0; index < corners.GetSize(); ++index) { corners[index]->SetBevel(TRUE); }
        return executive.DoFilleting();
    }
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
