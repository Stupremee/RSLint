impl Config {
    pub const fn preset() -> Self {
        Self {
            no_shadow: NoShadowConf {
                enabled: true,
                hoisting: NoShadowHoisting::HoistingNever,
            },
            no_undef: true,
            no_unused_labels: true,
            typeof_undef: true,
            unused_vars: true,
            use_before_def: true,
        }
    }
}
