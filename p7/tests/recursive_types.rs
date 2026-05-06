fn compile_source(source: &str) {
    p7::compile(source.to_string()).expect("source compiles");
}

#[test]
fn recursive_struct_compiles() {
    compile_source(
        r#"
pub struct Node(pub kind: int, pub children: array<Node>);
pub fn make_leaf() -> Node {
    let children: array<Node> = [];
    Node(0, children)
}
pub fn make_branch() -> Node { Node(1, [make_leaf(), make_leaf()]) }
"#,
    );
}

#[test]
fn recursive_enum_compiles() {
    compile_source(
        r#"
pub enum Tree(Leaf: int, Branch: array<Tree>);
pub fn make_tree() -> Tree { Tree.Branch([Tree.Leaf(1), Tree.Leaf(2)]) }
"#,
    );
}

#[test]
fn mutually_recursive_structs_compile() {
    compile_source(
        r#"
pub struct A(pub b: array<B>);
pub struct B(pub a: array<A>);
"#,
    );
}

#[test]
fn forward_reference_struct_compiles() {
    compile_source(
        r#"
pub struct A(pub b: B);
pub struct B(pub x: int);
"#,
    );
}
