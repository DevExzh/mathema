/// TeX math style system with 8 styles and transition rules.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MathStyle {
    Display,
    DisplayCramped,
    Text,
    TextCramped,
    Script,
    ScriptCramped,
    ScriptScript,
    ScriptScriptCramped,
}

impl MathStyle {
    /// Style for superscripts (TeXBook: D→S, T→S, S→SS, SS→SS; cramped stays cramped).
    pub fn sup(self) -> Self {
        match self {
            Self::Display | Self::Text => Self::Script,
            Self::DisplayCramped | Self::TextCramped => Self::ScriptCramped,
            Self::Script | Self::ScriptScript => Self::ScriptScript,
            Self::ScriptCramped | Self::ScriptScriptCramped => Self::ScriptScriptCramped,
        }
    }

    /// Style for subscripts (always cramped version of sup style).
    pub fn sub(self) -> Self {
        self.sup().cramped()
    }

    /// Style for fraction numerator (D→T, T→S, S→SS, SS→SS).
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
    pub fn fracden(self) -> Self {
        self.fracnum().cramped()
    }

    /// Cramped version of this style.
    pub fn cramped(self) -> Self {
        match self {
            Self::Display | Self::DisplayCramped => Self::DisplayCramped,
            Self::Text | Self::TextCramped => Self::TextCramped,
            Self::Script | Self::ScriptCramped => Self::ScriptCramped,
            Self::ScriptScript | Self::ScriptScriptCramped => Self::ScriptScriptCramped,
        }
    }

    pub fn is_cramped(self) -> bool {
        matches!(
            self,
            Self::DisplayCramped
                | Self::TextCramped
                | Self::ScriptCramped
                | Self::ScriptScriptCramped
        )
    }

    pub fn is_display(self) -> bool {
        matches!(self, Self::Display | Self::DisplayCramped)
    }

    /// Font size multiplier relative to base size.
    /// Display/Text = 1.0, Script = 0.7, ScriptScript = 0.5
    pub fn size_multiplier(self) -> f64 {
        match self {
            Self::Display | Self::DisplayCramped | Self::Text | Self::TextCramped => 1.0,
            Self::Script | Self::ScriptCramped => 0.7,
            Self::ScriptScript | Self::ScriptScriptCramped => 0.5,
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
}
