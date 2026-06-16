/// TeX typesetting parameters (FONTDIMEN values from Latin Modern / Computer Modern).
/// All values in em units relative to the base font size.
///
/// Style-dependent values (sigma parameters from cmsy10/cmsy7/cmsy5 and
/// xi parameters from cmex10/cmex7) vary by size class:
///   0 = text/display (cmsy10, cmex10)
///   1 = script       (cmsy7,  cmex7)
///   2 = scriptscript (cmsy5,  cmex7)

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
    /// Numerator shift up for no-bar fractions in non-display styles.
    pub num3: f64,
    /// Display-mode fraction denominator shift down.
    pub denom1: f64,
    /// Text-mode fraction denominator shift down.
    pub denom2: f64,
    /// Spacing for limits on big operators: [above_num, above_den, above_sep, below_sep, below_den].
    pub big_op_spacing: [f64; 5],
}

/// Style-dependent parameter tables from KaTeX's fontMetricsData.
/// Each array is [text/display, script, scriptscript].
/// Sources: cmsy10 (sigma, text), cmsy7 (sigma, script), cmsy5 (sigma, scriptscript),
///          cmex10 (xi, text), cmex7 (xi, script/scriptscript).
const SIGMA_SUP1: [f64; 3] = [0.413, 0.503, 0.504];
const SIGMA_SUP2: [f64; 3] = [0.363, 0.431, 0.404];
const SIGMA_SUP3: [f64; 3] = [0.289, 0.286, 0.294];
const SIGMA_SUB1: [f64; 3] = [0.150, 0.143, 0.200];
const SIGMA_SUB2: [f64; 3] = [0.247, 0.286, 0.400];
const SIGMA_SUP_DROP: [f64; 3] = [0.386, 0.353, 0.494];
const SIGMA_SUB_DROP: [f64; 3] = [0.050, 0.071, 0.100];
const SIGMA_NUM1: [f64; 3] = [0.677, 0.732, 0.925];
const SIGMA_NUM2: [f64; 3] = [0.394, 0.384, 0.387];
const SIGMA_NUM3: [f64; 3] = [0.444, 0.471, 0.504];
const SIGMA_DENOM1: [f64; 3] = [0.686, 0.752, 1.025];
const SIGMA_DENOM2: [f64; 3] = [0.345, 0.344, 0.532];
const SIGMA_DELIM1: [f64; 3] = [2.390, 1.700, 1.980];
const SIGMA_DELIM2: [f64; 3] = [1.010, 1.157, 1.420];
const XI_RULE: [f64; 3] = [0.040, 0.049, 0.049];
const SIGMA_QUAD: [f64; 3] = [1.000, 1.171, 1.472];
const XI_BIG_OP_SPACING: [[f64; 5]; 3] = [
    [0.111, 0.166, 0.200, 0.600, 0.100],
    [0.111, 0.166, 0.200, 0.611, 0.143],
    [0.111, 0.166, 0.200, 0.611, 0.143],
];

impl TexParams {
    /// Return TeX parameters resolved for the given size class.
    ///
    /// Size classes follow TeX/KaTeX convention:
    ///   0 = text/display (cmsy10 sigma, cmex10 xi)
    ///   1 = script       (cmsy7 sigma,  cmex7 xi)
    ///   2 = scriptscript (cmsy5 sigma,  cmex7 xi)
    #[must_use]
    pub fn for_size_class(idx: usize) -> Self {
        let i = idx.min(2);
        // Prefer MATH table values over hardcoded CM fallbacks.
        let mc = super::math_table::math_constants();
        Self {
            axis_height: mc.map_or(0.250, |c| c.axis_height),
            rule_thickness: XI_RULE[i],
            x_height: mc.map_or(0.431, |c| c.accent_base_height),
            quad: SIGMA_QUAD[i],
            sup1: SIGMA_SUP1[i],
            sup2: SIGMA_SUP2[i],
            sup3: SIGMA_SUP3[i],
            sub1: SIGMA_SUB1[i],
            sub2: SIGMA_SUB2[i],
            sup_drop: SIGMA_SUP_DROP[i],
            sub_drop: SIGMA_SUB_DROP[i],
            delim1: SIGMA_DELIM1[i],
            delim2: SIGMA_DELIM2[i],
            num1: SIGMA_NUM1[i],
            num2: SIGMA_NUM2[i],
            num3: SIGMA_NUM3[i],
            denom1: SIGMA_DENOM1[i],
            denom2: SIGMA_DENOM2[i],
            big_op_spacing: XI_BIG_OP_SPACING[i],
        }
    }
}

impl Default for TexParams {
    fn default() -> Self {
        // Text/display style values (size class 0)
        Self::for_size_class(0)
    }
}
