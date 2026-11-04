//! Interactive component groups follow source references, without merging placements.

use crate::{
    geometry::PathError,
    model::{BoardScene, ObjectId},
    selection::SelectedObject,
    task::CancellationToken,
};

/// A stable, typed source anchor for a reference group. Pin and via IDs may overlap.
/// The first pin, or first finger when there are no pins, represents the group.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ComponentAnchor {
    Pin(ObjectId),
    Finger(ObjectId),
}

impl ComponentAnchor {
    pub fn object(self) -> SelectedObject {
        match self {
            Self::Pin(id) => SelectedObject::Pin(id),
            Self::Finger(id) => SelectedObject::Via(id),
        }
    }

    pub fn id(self) -> ObjectId {
        match self {
            Self::Pin(id) | Self::Finger(id) => id,
        }
    }

    /// Empty references have no component group; missing anchors are source errors.
    pub fn reference<'a>(
        self,
        scene: &'a BoardScene,
        cancel: &CancellationToken,
    ) -> Result<Option<&'a str>, PathError> {
        fn find<'a>(
            values: impl Iterator<Item = (ObjectId, Option<&'a str>)>,
            expected: ObjectId,
            cancel: &CancellationToken,
        ) -> Result<Option<&'a str>, PathError> {
            for (id, reference) in values {
                cancel.check()?;
                if id == expected {
                    return Ok(reference.filter(|value| !value.is_empty()));
                }
            }
            cancel.check()?;
            Err(PathError::Invalid(expected))
        }
        match self {
            Self::Pin(id) => find(
                scene
                    .pins
                    .iter()
                    .map(|pin| (pin.id, Some(pin.reference.as_str()))),
                id,
                cancel,
            ),
            Self::Finger(id) => find(
                scene.vias.iter().map(|via| {
                    (
                        via.id,
                        via.finger.as_ref().map(|finger| finger.reference.as_str()),
                    )
                }),
                id,
                cancel,
            ),
        }
    }

    /// Canonicalize all same-reference source anchors to one interactive identity.
    /// Reference comparison is exact and is independent of UI locale.
    pub fn from_object(
        object: SelectedObject,
        scene: &BoardScene,
        cancel: &CancellationToken,
    ) -> Result<Option<Self>, PathError> {
        if let SelectedObject::Placement(id) = object {
            for component in &scene.components {
                cancel.check()?;
                if component.id == id {
                    return Self::for_reference(&component.reference, scene, cancel);
                }
            }
            return Err(PathError::Invalid(id));
        }
        let anchor = match object {
            SelectedObject::Pin(id) => Self::Pin(id),
            SelectedObject::Via(id) => Self::Finger(id),
            _ => {
                cancel.check()?;
                return Ok(None);
            }
        };
        let Some(reference) = anchor.reference(scene, cancel)? else {
            return Ok(None);
        };
        Self::for_reference(reference, scene, cancel)
    }

    pub fn for_reference(
        reference: &str,
        scene: &BoardScene,
        cancel: &CancellationToken,
    ) -> Result<Option<Self>, PathError> {
        cancel.check()?;
        if reference.is_empty() {
            return Ok(None);
        }
        for pin in &scene.pins {
            cancel.check()?;
            if pin.reference == reference {
                return Ok(Some(Self::Pin(pin.id)));
            }
        }
        for via in &scene.vias {
            cancel.check()?;
            if via
                .finger
                .as_ref()
                .is_some_and(|finger| finger.reference == reference)
            {
                return Ok(Some(Self::Finger(via.id)));
            }
        }
        Ok(None)
    }
}
