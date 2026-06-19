//! TeX math style system with 8 styles and transition rules.

/// One of TeX's eight math styles (display/text/script/scriptscript, each with a
/// cramped variant) that controls sizing and layout of sub-expressions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathStyle {
    /// Display style (`\displaystyle`), used for standalone equations.
    Display,
    /// Cramped display style (e.g. under a radical).
    DisplayCramped,
    /// Text style (`\textstyle`), used for inline math.
    Text,
    /// Cramped text style.
    TextCramped,
    /// Script style (`\scriptstyle`), used for first-level scripts.
    Script,
    /// Cramped script style.
    ScriptCramped,
    /// Script-script style (`\scriptscriptstyle`), used for nested scripts.
    ScriptScript,
    /// Cramped script-script style.
    ScriptScriptCramped,
}

impl MathStyle {
    /// Style for superscripts (TeXBook: D→S, T→S, S→SS, SS→SS; cramped stays cramped).
    #[must_use]
    #[inline]
    pub fn sup(self) -> Self {
        match self {
            Self::Display | Self::Text => Self::Script,
            Self::DisplayCramped | Self::TextCramped => Self::ScriptCramped,
            Self::Script | Self::ScriptScript => Self::ScriptScript,
            Self::ScriptCramped | Self::ScriptScriptCramped => Self::ScriptScriptCramped,
        }
    }

    /// Style for subscripts (always cramped version of sup style).
    #[must_use]
    #[inline]
    pub fn sub(self) -> Self {
        self.sup().cramped()
    }

    /// Style for fraction numerator (D→T, T→S, S→SS, SS→SS).
    #[must_use]
    #[inline]
    pub fn fracnum(self) -> Self {
        match self {
            Self::Display => Self::Text,
            Self::DisplayCramped => Self::TextCramped,
            Self::Text => Self::Script,
            Self::TextCramped => Self::ScriptCramped,
            Self::Script | Self::ScriptScript => Self::ScriptScript,
            Self::ScriptCramped | Self::ScriptScriptCramped => Self::ScriptScriptCramped,
        }
    }

    /// Style for fraction denominator (always cramped version of fracnum).
    #[must_use]
    #[inline]
    pub fn fracden(self) -> Self {
        self.fracnum().cramped()
    }

    /// Cramped version of this style.
    #[must_use]
    #[inline]
    pub fn cramped(self) -> Self {
        match self {
            Self::Display | Self::DisplayCramped => Self::DisplayCramped,
            Self::Text | Self::TextCramped => Self::TextCramped,
            Self::Script | Self::ScriptCramped => Self::ScriptCramped,
            Self::ScriptScript | Self::ScriptScriptCramped => Self::ScriptScriptCramped,
        }
    }

    /// Return `true` if this is a cramped style.
    #[must_use]
    #[inline]
    pub fn is_cramped(self) -> bool {
        matches!(
            self,
            Self::DisplayCramped
                | Self::TextCramped
                | Self::ScriptCramped
                | Self::ScriptScriptCramped
        )
    }

    /// Return `true` if this is a display style (cramped or not).
    #[must_use]
    #[inline]
    pub fn is_display(self) -> bool {
        matches!(self, Self::Display | Self::DisplayCramped)
    }

    /// Font size multiplier relative to base size.
    /// Display/Text = 1.0, Script = 0.7, ScriptScript = 0.5
    #[must_use]
    #[inline]
    pub fn size_multiplier(self) -> f64 {
        match self {
            Self::Display | Self::DisplayCramped | Self::Text | Self::TextCramped => 1.0,
            Self::Script | Self::ScriptCramped => 0.7,
            Self::ScriptScript | Self::ScriptScriptCramped => 0.5,
        }
    }

    /// Size class index for style-dependent TeX parameters.
    /// 0 = text/display (cmsy10), 1 = script (cmsy7), 2 = scriptscript (cmsy5).
    #[must_use]
    #[inline]
    pub fn size_class(self) -> usize {
        match self {
            Self::Display | Self::DisplayCramped | Self::Text | Self::TextCramped => 0,
            Self::Script | Self::ScriptCramped => 1,
            Self::ScriptScript | Self::ScriptScriptCramped => 2,
        }
    }

    /// Clamp this style to at least text level for spacing purposes.
    /// Script/ScriptScript → TextCramped; Display/Text pass through unchanged.
    /// This ensures fraction content always receives at least text-level
    /// inter-atom spacing (medium and thick spaces around binary/relation ops).
    #[must_use]
    #[inline]
    pub fn min_text_level(self) -> Self {
        match self {
            Self::Script | Self::ScriptCramped | Self::ScriptScript | Self::ScriptScriptCramped => {
                Self::TextCramped
            },
            other => other,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sup_transitions() {
        assert_eq!(MathStyle::Display.sup(), MathStyle::Script);
        assert_eq!(MathStyle::Text.sup(), MathStyle::Script);
        assert_eq!(MathStyle::Script.sup(), MathStyle::ScriptScript);
        assert_eq!(MathStyle::ScriptScript.sup(), MathStyle::ScriptScript);
        assert_eq!(MathStyle::DisplayCramped.sup(), MathStyle::ScriptCramped);
    }

    #[test]
    fn sub_is_cramped_sup() {
        assert_eq!(MathStyle::Display.sub(), MathStyle::ScriptCramped);
        assert_eq!(MathStyle::Text.sub(), MathStyle::ScriptCramped);
        assert_eq!(MathStyle::Script.sub(), MathStyle::ScriptScriptCramped);
    }

    #[test]
    fn fracnum_transitions() {
        assert_eq!(MathStyle::Display.fracnum(), MathStyle::Text);
        assert_eq!(MathStyle::Text.fracnum(), MathStyle::Script);
        assert_eq!(MathStyle::Script.fracnum(), MathStyle::ScriptScript);
    }

    #[test]
    fn fracden_is_cramped_fracnum() {
        assert_eq!(MathStyle::Display.fracden(), MathStyle::TextCramped);
        assert_eq!(MathStyle::Text.fracden(), MathStyle::ScriptCramped);
    }

    #[test]
    fn cramped_idempotent() {
        assert_eq!(
            MathStyle::DisplayCramped.cramped(),
            MathStyle::DisplayCramped
        );
        assert_eq!(MathStyle::Display.cramped(), MathStyle::DisplayCramped);
    }

    #[test]
    fn size_multipliers() {
        assert_eq!(MathStyle::Display.size_multiplier(), 1.0);
        assert_eq!(MathStyle::Text.size_multiplier(), 1.0);
        assert_eq!(MathStyle::Script.size_multiplier(), 0.7);
        assert_eq!(MathStyle::ScriptScript.size_multiplier(), 0.5);
    }

    #[test]
    fn is_display_flags() {
        assert!(MathStyle::Display.is_display());
        assert!(MathStyle::DisplayCramped.is_display());
        assert!(!MathStyle::Text.is_display());
        assert!(!MathStyle::Script.is_display());
    }

    #[test]
    fn min_text_level_clamps_script() {
        // Display/Text pass through unchanged
        assert_eq!(MathStyle::Display.min_text_level(), MathStyle::Display);
        assert_eq!(
            MathStyle::DisplayCramped.min_text_level(),
            MathStyle::DisplayCramped
        );
        assert_eq!(MathStyle::Text.min_text_level(), MathStyle::Text);
        assert_eq!(
            MathStyle::TextCramped.min_text_level(),
            MathStyle::TextCramped
        );
        // Script/ScriptScript are clamped to TextCramped
        assert_eq!(MathStyle::Script.min_text_level(), MathStyle::TextCramped);
        assert_eq!(
            MathStyle::ScriptCramped.min_text_level(),
            MathStyle::TextCramped
        );
        assert_eq!(
            MathStyle::ScriptScript.min_text_level(),
            MathStyle::TextCramped
        );
        assert_eq!(
            MathStyle::ScriptScriptCramped.min_text_level(),
            MathStyle::TextCramped
        );
    }
}
