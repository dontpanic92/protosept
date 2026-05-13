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
fn gc_compaction_updates_box_refs_inside_shared_arrays() {
    let mut ctx = Context::new();
    ctx.box_heap.push(Data::Int(0));
    ctx.box_heap.push(Data::Int(7));

    let shared = Data::array(vec![Data::BoxRef(1)]);
    let external_root = ctx.add_external_root(shared.clone());
    ctx.stack[0].stack.push(shared);

    ctx.collect_garbage().expect("gc should compact");

    assert_eq!(ctx.box_heap, vec![Data::Int(7)]);
    for root in [
        ctx.stack[0].stack.last().expect("stack root"),
        ctx.external_root(external_root)
            .as_ref()
            .expect("external root"),
    ] {
        let elements = root.array_elements().expect("array root");
        assert_eq!(elements, &[Data::BoxRef(0)]);
    }
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
