use egg::{rewrite as rw, *};

use log::trace;
use ordered_float::NotNan;

pub type EGraph = egg::EGraph<Math, Meta>;
pub type Rewrite = egg::Rewrite<Math, Meta>;

type Constant = NotNan<f64>;

define_language! {
    pub enum Math {
        Diff = "d",

        Constant(Constant),
        Add = "+",
        Sub = "-",
        Mul = "*",
        Div = "/",
        Pow = "pow",
        Exp = "exp",
        Log = "log",
        Sqrt = "sqrt",
        Cbrt = "cbrt",
        Fabs = "fabs",

        Log1p = "log1p",
        Expm1 = "expm1",

        RealToPosit = "real->posit",
        Variable(String),
    }
}

// You could use egg::AstSize, but this is useful for debugging, since
// it will really try to get rid of the Diff operator
struct MathCostFn;
impl egg::CostFunction<Math> for MathCostFn {
    type Cost = usize;
    fn cost(&mut self, enode: &ENode<Math, Self::Cost>) -> Self::Cost {
        let op_cost = match enode.op {
            Math::Diff => 100,
            _ => 1,
        };
        op_cost + enode.children.iter().sum::<usize>()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Meta {
    pub cost: usize,
    pub best: RecExpr<Math>,
}

fn eval(op: Math, args: &[Constant]) -> Option<Constant> {
    let a = |i| args.get(i).cloned();
    trace!("{} {:?} = ...", op, args);
    let zero = Some(0.0.into());
    let res = match op {
        Math::Add => Some(a(0)? + a(1)?),
        Math::Sub => Some(a(0)? - a(1)?),
        Math::Mul => Some(a(0)? * a(1)?),
        Math::Div if a(1) != zero => Some(a(0)? / a(1)?),
        _ => None,
    };
    trace!("{} {:?} = {:?}", op, args, res);
    res
}

impl Metadata<Math> for Meta {
    type Error = std::convert::Infallible;
    fn merge(&self, other: &Self) -> Self {
        if self.cost <= other.cost {
            self.clone()
        } else {
            other.clone()
        }
    }

    fn make(egraph: &EGraph, enode: &ENode<Math>) -> Self {
        let meta = |i: Id| &egraph[i].metadata;
        let enode = {
            let const_args: Option<Vec<Constant>> = enode
                .children
                .iter()
                .map(|id| match meta(*id).best.as_ref().op {
                    Math::Constant(c) => Some(c),
                    _ => None,
                })
                .collect();

            const_args
                .and_then(|a| eval(enode.op.clone(), &a))
                .map(|c| ENode::leaf(Math::Constant(c)))
                .unwrap_or_else(|| enode.clone())
        };

        let best: RecExpr<_> = enode.map_children(|c| meta(c).best.clone()).into();
        let cost = MathCostFn.cost(&enode.map_children(|c| meta(c).cost));
        Self { best, cost }
    }

    fn modify(eclass: &mut EClass<Math, Self>) {
        // NOTE pruning vs not pruning is decided right here
        // not pruning would be just pushing instead of replacing
        let best = eclass.metadata.best.as_ref();
        if best.children.is_empty() {
            eclass.nodes = vec![ENode::leaf(best.op.clone())]
        }
    }
}

fn c_is_const_or_var_and_not_x(egraph: &mut EGraph, _: Id, subst: &Subst) -> bool {
    let c = "?c".parse().unwrap();
    let x = "?x".parse().unwrap();
    let is_const_or_var = egraph[subst[&c]].nodes.iter().any(|n| match n.op {
        Math::Constant(_) | Math::Variable(_) => true,
        _ => false,
    });
    is_const_or_var && subst[&x] != subst[&c]
}

fn is_not_zero(var: &'static str) -> impl Fn(&mut EGraph, Id, &Subst) -> bool {
    let var = var.parse().unwrap();
    let zero = enode!(Math::Constant(0.0.into()));
    move |egraph, _, subst| !egraph[subst[&var]].nodes.contains(&zero)
}

#[rustfmt::skip]
pub fn rules() -> Vec<Rewrite> { vec![
    rw!("comm-add";  "(+ ?a ?b)"        => "(+ ?b ?a)"),
    rw!("comm-mul";  "(* ?a ?b)"        => "(* ?b ?a)"),
    rw!("assoc-add"; "(+ ?a (+ ?b ?c))" => "(+ (+ ?a ?b) ?c)"),
    rw!("assoc-mul"; "(* ?a (* ?b ?c))" => "(* (* ?a ?b) ?c)"),

    rw!("sub-canon"; "(- ?a ?b)" => "(+ ?a (* -1 ?b))"),
    rw!("div-canon"; "(/ ?a ?b)" => "(* ?a (pow ?b -1))"),
    rw!("canon-sub"; "(+ ?a (* -1 ?b))"   => "(- ?a ?b)"),
    // rw!("canon-div"; "(* ?a (pow ?b -1))" => "(/ ?a ?b)" if is_not_zero("?b")),

    rw!("zero-add"; "(+ ?a 0)" => "?a"),
    rw!("zero-mul"; "(* ?a 0)" => "0"),
    rw!("one-mul";  "(* ?a 1)" => "?a"),

    rw!("add-zero"; "?a" => "(+ ?a 0)"),
    rw!("mul-one";  "?a" => "(* ?a 1)"),

    rw!("cancel-sub"; "(- ?a ?a)" => "0"),
    rw!("cancel-div"; "(/ ?a ?a)" => "1"),

    rw!("distribute"; "(* ?a (+ ?b ?c))"        => "(+ (* ?a ?b) (* ?a ?c))"),
    rw!("factor"    ; "(+ (* ?a ?b) (* ?a ?c))" => "(* ?a (+ ?b ?c))"),

    rw!("pow-intro"; "?a" => "(pow ?a 1)"),
    rw!("pow-mul"; "(* (pow ?a ?b) (pow ?a ?c))" => "(pow ?a (+ ?b ?c))"),
    rw!("pow0"; "(pow ?x 0)" => "1"),
    rw!("pow1"; "(pow ?x 1)" => "?x"),
    rw!("pow2"; "(pow ?x 2)" => "(* ?x ?x)"),
    rw!("pow-recip"; "(pow ?x -1)" => "(/ 1 ?x)" if is_not_zero("?x")),

    rw!("d-variable"; "(d ?x ?x)" => "1"),
    rw!("d-constant"; "(d ?x ?c)" => "0" if c_is_const_or_var_and_not_x),

    rw!("d-add"; "(d ?x (+ ?a ?b))" => "(+ (d ?x ?a) (d ?x ?b))"),
    rw!("d-mul"; "(d ?x (* ?a ?b))" => "(+ (* ?a (d ?x ?b)) (* ?b (d ?x ?a)))"),

    rw!("d-power";
        "(d ?x (pow ?f ?g))" =>
        "(* (pow ?f ?g)
            (+ (* (d ?x ?f)
                  (/ ?g ?f))
               (* (d ?x ?g)
                  (log ?f))))"
        if is_not_zero("?f")
    ),
]}

egg::test_fn! {
    #[cfg_attr(feature = "parent-pointers", ignore)]
    math_associate_adds, [
        rw!("comm-add"; "(+ ?a ?b)" => "(+ ?b ?a)"),
        rw!("assoc-add"; "(+ ?a (+ ?b ?c))" => "(+ (+ ?a ?b) ?c)"),
    ],
    runner = Runner::new()
        .with_iter_limit(7)
        .with_scheduler(SimpleScheduler),
    "(+ 1 (+ 2 (+ 3 (+ 4 (+ 5 (+ 6 7))))))"
    =>
    "(+ 7 (+ 6 (+ 5 (+ 4 (+ 3 (+ 2 1))))))"
    @check |r: Runner<Math, ()>| assert_eq!(r.egraph.number_of_classes(), 127)
}

egg::test_fn! {
    #[should_panic(expected = "Could not prove goal 0")]
    math_fail, rules(),
    "(+ x y)" => "(/ x y)"
}

egg::test_fn! {math_simplify_add, rules(), "(+ x (+ x (+ x x)))" => "(* 4 x)" }
egg::test_fn! {math_powers, rules(), "(* (pow 2 x) (pow 2 y))" => "(pow 2 (+ x y))"}

egg::test_fn! {
    #[cfg_attr(feature = "parent-pointers", ignore)]
    math_simplify_const, rules(),
    "(+ 1 (- a (* (- 2 1) a)))" => "1"
}

egg::test_fn! {
    #[cfg_attr(feature = "parent-pointers", ignore)]
    math_simplify_root, rules(),
    runner = Runner::new().with_node_limit(75_000),
    r#"
    (/ 1
       (- (/ (+ 1 (sqrt five))
             2)
          (/ (- 1 (sqrt five))
             2)))"#
    =>
    "(/ 1 (sqrt five))"
}

egg::test_fn! {math_diff_same,      rules(), "(d x x)" => "1"}
egg::test_fn! {math_diff_different, rules(), "(d x y)" => "0"}
egg::test_fn! {math_diff_simple1,   rules(), "(d x (+ 1 (* 2 x)))" => "2"}
egg::test_fn! {math_diff_simple2,   rules(), "(d x (+ 1 (* y x)))" => "y"}

egg::test_fn! {
    #[cfg_attr(feature = "parent-pointers", ignore)]
    diff_power_simple, rules(),
    "(d x (pow x 3))" => "(* 3 (pow x 2))"
}
egg::test_fn! {
    #[cfg_attr(feature = "parent-pointers", ignore)]
    diff_power_harder, rules(),
    runner = Runner::new()
        .with_iter_limit(50)
        .with_node_limit(50_000)
        // HACK this needs to "see" the end expression
        .with_expr(&"(* x (- (* 3 x) 14))".parse().unwrap()),
    "(d x (- (pow x 3) (* 7 (pow x 2))))"
    =>
    "(* x (- (* 3 x) 14))"
}

#[test]
fn ac_match() {
    let _ = env_logger::builder().is_test(true).try_init();
    let start = &"(* (+ a (+ b c)) (- (+ (* a a) (+ (* b b) (* c c))) (+ (* a b) (+ (* b c) (* a c)))))".parse().unwrap();
    let end = &"(- (+ (pow a 3) (+ (pow b 3) (pow c 3))) (* 3 (* a (* b c))))".parse().unwrap();

    let rules = rules();
    let mut runner = Runner::new()
        .with_iter_limit(1000)
        .with_node_limit(500_000)
        .with_time_limit(std::time::Duration::from_secs(60));
    let start_c = runner.egraph.add_expr(start);
    let end_c = runner.egraph.add_expr(end);
    println!("{:?}", (start_c, end_c));
    let res = runner.run(&rules);
    let sc = res.egraph.find(start_c);
    let ec = res.egraph.find(end_c);
    println!("{:?}", (sc, ec));
    println!(
        "Stopped after {} iterations, reason: {:?}",
        res.iterations.len(),
        res.stop_reason
    );
}

// egg::test_fn! {
//     ac_match, rules(),
//     runner = Runner::new()
//         .with_iter_limit(100)
//         .with_node_limit(500_000)
//         .with_expr(&"(- (+ (pow a 3) (+ (pow b 3) (pow c 3))) (* 3 (* a (* b c))))".parse().unwrap()),
//     "(* (+ a (+ b c)) (- (+ (* a a) (+ (* b b) (* c c))) (+ (* a b) (+ (* b c) (* a c)))))"
//     =>
//     "(- (+ (pow a 3) (+ (pow b 3) (pow c 3))) (* 3 (* a (* b c))))"
// }
