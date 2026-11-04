use pomelo_core::model::LayerId;

/// Layer IDs are persisted in saved view state; these values must never change.
#[test]
fn layer_id_ranges_keep_their_persisted_values() {
    assert_eq!(LayerId::allegro_drawing(0x0102), LayerId(0x10102));
    assert_eq!(LayerId::DIMENSION, LayerId(0x1f901));
    assert_eq!(LayerId::special(7), LayerId(0x20007));
    assert_eq!(LayerId::BOND_TOP, LayerId(0x20000));
    assert_eq!(LayerId::BOND_WIRE_TOP, LayerId(0x20001));
    assert_eq!(LayerId::odb_graphic(3), LayerId(0x30003));
}

#[test]
fn layer_classification_follows_the_ranges() {
    assert!(LayerId(0xffff).is_copper());
    assert!(!LayerId::allegro_drawing(0).is_copper());
    assert!(!LayerId::allegro_drawing(0xffff).is_special());
    assert!(LayerId::special(0).is_special());
    assert!(LayerId::odb_graphic(0).is_special());
    assert!(!LayerId::UNASSIGNED.is_copper());
}
