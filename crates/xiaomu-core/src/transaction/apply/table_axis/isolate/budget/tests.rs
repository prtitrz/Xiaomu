use super::*;

#[test]
fn child_id_vectors_and_generated_nodes_are_charged_with_checked_arithmetic() {
    let mut budget = OwnedBudget::default();
    assert_eq!(
        budget.scaled((size_of::<NodeId>(), 1), usize::MAX),
        Err(Error::TableResourceLimit)
    );
    let mut budget = OwnedBudget::default();
    assert_eq!(
        budget.nodes(MAX_EDIT_NODES + 1),
        Err(Error::TableResourceLimit)
    );
    let mut budget = OwnedBudget {
        bytes: MAX_OUTPUT_ATTR_BYTES - 1,
        ..Default::default()
    };
    assert_eq!(
        budget.scaled((size_of::<NodeId>(), 1), 1),
        Err(Error::TableResourceLimit)
    );
    let mut budget = OwnedBudget {
        values: MAX_OUTPUT_ATTR_VALUES,
        ..Default::default()
    };
    assert_eq!(budget.scaled((0, 1), 1), Err(Error::TableResourceLimit));
    assert_eq!(add(usize::MAX, 1), Err(Error::TableResourceLimit));
}
