pub fn material_physics(material: u32) -> (f32, f32, f32, f32, f32) {
    let m = rbx_math::materials::material_physics(material);
    (
        m.density,
        m.friction,
        m.elasticity,
        m.friction_weight,
        m.elasticity_weight,
    )
}
