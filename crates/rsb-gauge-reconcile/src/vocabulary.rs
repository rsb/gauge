//! The closed vocabularies stored as text. Each reads only its own members;
//! anything else is [`ReconcileError::Unknown`](crate::ReconcileError).

/// A fieldless enum with one text form per member, read and written through
/// that text and through serde.
macro_rules! vocabulary {
    (
        $(#[$meta:meta])*
        pub enum $name:ident as $label:literal {
            $($(#[$vmeta:meta])* $variant:ident = $text:literal,)+
        }
    ) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub enum $name {
            $($(#[$vmeta])* $variant,)+
        }

        impl $name {
            /// Every member, in declaration order.
            pub const ALL: &[Self] = &[$(Self::$variant,)+];

            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text,)+
                }
            }
        }

        impl std::str::FromStr for $name {
            type Err = $crate::ReconcileError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                match s {
                    $($text => Ok(Self::$variant),)+
                    _ => Err($crate::ReconcileError::Unknown {
                        vocabulary: $label,
                        value: s.to_owned(),
                    }),
                }
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(self.as_str())
            }
        }

        impl serde::Serialize for $name {
            fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
                serializer.serialize_str(self.as_str())
            }
        }

        impl<'de> serde::Deserialize<'de> for $name {
            fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
                let text = String::deserialize(deserializer)?;
                text.parse().map_err(serde::de::Error::custom)
            }
        }
    };
}

pub(crate) use vocabulary;
