/// TeX typesetting parameters (FONTDIMEN values from Latin Modern / Computer Modern).
/// All values in em units relative to the base font size.

#[derive(Debug, Clone, Copy)]
pub struct TexParams {
    /// Height of the math axis above baseline.
    pub axis_height: f64,
    /// Default thickness of fraction bars and rules.
    pub rule_thickness: f64,
    /// Height of lowercase 'x'.
    pub x_height: f64,
    /// Width of one em.
    pub quad: f64,
    /// Superscript shift in display style.
    pub sup1: f64,
    /// Superscript shift in text style.
    pub sup2: f64,
    /// Superscript shift in cramped styles.
    pub sup3: f64,
    /// Subscript shift down.
    pub sub1: f64,
    /// Subscript shift when superscript is present.
    pub sub2: f64,
    /// Amount to drop superscript below top of base.
    pub sup_drop: f64,
    /// Amount to drop subscript below bottom of base.
    pub sub_drop: f64,
    /// Display-mode delimiter minimum size.
    pub delim1: f64,
    /// Text-mode delimiter minimum size.
    pub delim2: f64,
    /// Display-mode fraction numerator shift up.
    pub num1: f64,
    /// Text-mode fraction numerator shift up.
    pub num2: f64,
    /// Display-mode fraction denominator shift down.
    pub denom1: f64,
    /// Text-mode fraction denominator shift down.
    pub denom2: f64,
    /// Spacing for limits on big operators: [above_num, above_den, above_sep, below_sep, below_den].
    pub big_op_spacing: [f64; 5],
}

impl Default for TexParams {
    fn default() -> Self {
        // Values from Computer Modern / Latin Modern (cmr10, cmsy10, cmex10)
        Self {
            axis_height: 0.250,
            rule_thickness: 0.060,
            x_height: 0.431,
            quad: 1.000,
            sup1: 0.413,
            sup2: 0.363,
            sup3: 0.289,
            sub1: 0.150,
            sub2: 0.247,
            sup_drop: 0.386,
            sub_drop: 0.050,
            delim1: 2.390,
            delim2: 1.010,
            num1: 0.677,
            num2: 0.394,
            denom1: 0.686,
            denom2: 0.345,
            big_op_spacing: [0.111, 0.167, 0.200, 0.600, 0.100],
        }
    }
}
