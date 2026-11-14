//! The looks a picture can be drawn through, and what each does to it: plain numbers the shader
//! takes ([`crate::look_painter`]), so a look is the same on the live picture and on a photo.

/// One way the camera's picture is shown and its photos are kept.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Look {
    /// The picture as the camera gives it.
    #[default]
    Original,
    Mono,
    Warm,
    Cool,
    Vivid,
    Soft,
}

/// What a look does to a picture. [`Grade::NONE`] leaves it as it is.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct Grade {
    /// What red, green and blue are each multiplied by: the white balance.
    pub(crate) tint: [f32; 3],
    /// How much colour is kept: none at 0, all at 1, richer above.
    pub(crate) saturation: f32,
    /// How far light and dark are pushed apart about the middle grey: none at 0.
    pub(crate) contrast: f32,
    /// How much of the picture is its own blur: none at 0.
    pub(crate) softness: f32,
    /// How far the corners are darkened: none at 0.
    pub(crate) vignette: f32,
}

impl Grade {
    pub(crate) const NONE: Grade = Grade {
        tint: [1.0, 1.0, 1.0],
        saturation: 1.0,
        contrast: 0.0,
        softness: 0.0,
        vignette: 0.0,
    };
}

impl Look {
    /// Every look, in the order the app offers them.
    pub(crate) const ALL: [Look; 6] = [
        Look::Original,
        Look::Mono,
        Look::Warm,
        Look::Cool,
        Look::Vivid,
        Look::Soft,
    ];

    /// What the app calls the look.
    pub(crate) fn name(self) -> &'static str {
        match self {
            Look::Original => "Original",
            Look::Mono => "Mono",
            Look::Warm => "Warm",
            Look::Cool => "Cool",
            Look::Vivid => "Vivid",
            Look::Soft => "Soft",
        }
    }

    /// The look `by` places after this one in [`Look::ALL`], held at the first and the last.
    pub(crate) fn stepped(self, by: isize) -> Look {
        let at = Look::ALL.iter().position(|look| *look == self).unwrap_or(0);
        let last = Look::ALL.len() - 1;
        Look::ALL[at.saturating_add_signed(by).min(last)]
    }

    pub(crate) fn grade(self) -> Grade {
        match self {
            Look::Original => Grade::NONE,
            Look::Mono => Grade {
                saturation: 0.0,
                contrast: 0.25,
                ..Grade::NONE
            },
            Look::Warm => Grade {
                tint: [1.06, 1.0, 0.89],
                saturation: 1.05,
                ..Grade::NONE
            },
            Look::Cool => Grade {
                tint: [0.90, 0.99, 1.10],
                saturation: 0.95,
                ..Grade::NONE
            },
            Look::Vivid => Grade {
                saturation: 1.35,
                contrast: 0.45,
                ..Grade::NONE
            },
            Look::Soft => Grade {
                softness: 0.65,
                vignette: 0.3,
                ..Grade::NONE
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_original_look_leaves_the_picture_as_it_is() {
        assert_eq!(Look::default(), Look::Original);
        assert_eq!(Look::Original.grade(), Grade::NONE);
    }

    #[test]
    fn stepping_goes_through_the_looks_and_stops_at_the_ends() {
        assert_eq!(Look::Original.stepped(1), Look::Mono);
        assert_eq!(Look::Mono.stepped(-1), Look::Original);
        assert_eq!(Look::Original.stepped(-1), Look::Original);
        assert_eq!(Look::Soft.stepped(1), Look::Soft);
    }

    #[test]
    fn every_other_look_changes_the_picture_and_has_a_name_of_its_own() {
        let mut names = Vec::new();
        for look in Look::ALL {
            if look != Look::Original {
                assert_ne!(look.grade(), Grade::NONE, "{look:?}");
            }
            assert!(!names.contains(&look.name()), "{look:?}");
            names.push(look.name());
        }
    }
}
