use p7::interpreter::context::{Context, Data};

#[test]
fn shared_array_mutation_preserves_value_semantics() {
    let original = Data::array(vec![Data::Int(1)]);
    let mut boxed_value = original.clone();

    if let Data::Array(elements) = &mut boxed_value {
        std::rc::Rc::make_mut(elements).push(Data::Int(2));
    } else {
        panic!("expected array");
    }

    let original_elements = original.array_elements().expect("original array");
    let boxed_elements = boxed_value.array_elements().expect("boxed array");
    assert_eq!(original_elements, &[Data::Int(1)]);
    assert_eq!(boxed_elements, &[Data::Int(1), Data::Int(2)]);
}

#[test]
fn gc_keeps_box_indices_stable_across_collection() {
    // The box heap is a stable-handle slab — GC frees unmarked slots in
    // place but never moves a live slot. References inside Rust-owned
    // Data values stay valid across `collect_garbage`.
    let mut ctx = Context::new();
    let (live_idx, live_generation) = ctx.box_heap.alloc(Data::Int(7));
    let (dead_idx, _dead_generation) = ctx.box_heap.alloc(Data::Int(99));
    let _ = dead_idx;

    let shared = Data::array(vec![Data::BoxRef {
        idx: live_idx,
        generation: live_generation,
    }]);
    let external_root = ctx.add_external_root(shared.clone());
    ctx.stack[0].stack.push(shared);

    ctx.collect_garbage().expect("gc should run");

    // The live box is still at its original slot index — no compaction.
    assert_eq!(
        ctx.box_heap
            .get(live_idx, live_generation)
            .expect("live box should survive"),
        &Data::Int(7)
    );

    for root in [
        ctx.stack[0].stack.last().expect("stack root").clone(),
        ctx.external_root(external_root).expect("external root"),
    ] {
        let elements = root.array_elements().expect("array root").to_vec();
        assert_eq!(
            elements,
            vec![Data::BoxRef {
                idx: live_idx,
                generation: live_generation
            }]
        );
    }
}

#[test]
fn freed_slot_reuse_yields_stale_handle_error() {
    // Allocate a box, free it, then allocate a new value into the
    // recycled slot. The stale `Data::BoxRef` from the first allocation
    // should fail dereference with `StaleBoxHandle`, not silently alias
    // the new value.
    let mut ctx = Context::new();
    let (idx, generation) = ctx.box_heap.alloc(Data::Int(100));
    let stale_ref = Data::BoxRef { idx, generation };

    ctx.box_heap.free(idx);
    let (reused_idx, reused_generation) = ctx.box_heap.alloc(Data::Int(200));
    assert_eq!(
        reused_idx, idx,
        "free-list should reuse the freed slot before growing"
    );
    assert_ne!(
        reused_generation, generation,
        "freeing must bump the slot generation"
    );

    let stale_lookup = if let Data::BoxRef { idx, generation } = stale_ref {
        ctx.box_heap.get(idx, generation)
    } else {
        unreachable!()
    };
    assert!(
        matches!(
            stale_lookup,
            Err(p7::errors::RuntimeError::StaleBoxHandle { .. })
        ),
        "stale handle to recycled slot should fail with StaleBoxHandle, got {:?}",
        stale_lookup
    );

    // The fresh handle still works.
    assert_eq!(
        ctx.box_heap
            .get(reused_idx, reused_generation)
            .expect("fresh handle"),
        &Data::Int(200)
    );
}

#[test]
fn shared_nullable_payloads_clone_shallowly_but_compare_by_value() {
    let nested = Data::some(Data::array(vec![Data::string("value")]));
    let cloned = nested.clone();

    assert_eq!(nested, cloned);
    let inner = cloned.as_some().expect("nullable payload");
    let elements = inner.array_elements().expect("array payload");
    assert_eq!(elements, &[Data::string("value")]);
}
