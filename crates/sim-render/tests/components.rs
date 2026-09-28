use sim_core::{Engine, Loaded};
use sim_render::{Assets, Canvas, Registry, Scene, Theme, Ui, View};
use sim_rules::Game;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../games/colony");

type Setup = (Engine<sim_core::Running, Game>, View, Vec<sim_render::Projection>);

fn setup(view_src: &str) -> Result<Setup, Vec<String>> {
    let (world, game) = Game::load(std::path::Path::new(DIR), None).map_err(|e| vec![e])?;
    let engine = Engine::<Loaded, _>::new(world, game).validate()?.start();
    let mut view: View = ron::from_str(view_src).map_err(|e| vec![e.to_string()])?;
    let worlds = view.resolve(&Registry::default())?;
    Ok((engine, view, worlds))
}

fn ui(worlds: Vec<sim_render::Projection>) -> Ui {
    Ui {
        tick: 0,
        speed: 1.0,
        paused: false,
        outcome: None,
        selected: None,
        history: Default::default(),
        events: Default::default(),
        fps: 0.0,
        worlds,
    }
}

#[test]
fn the_colony_view_draws_from_data() {
    let src = std::fs::read_to_string(format!("{DIR}/view.ron")).unwrap();
    let (engine, view, worlds) = setup(&src).expect("valid view");
    assert!(!worlds.is_empty());
    let assets: Assets = ron::from_str(&std::fs::read_to_string(format!("{DIR}/../../assets/ants.ron")).unwrap()).unwrap();
    let scene = Scene { world: engine.world(), game: engine.rules(), assets: &assets };
    let mut canvas = Canvas::new(120, 40);
    view.draw(&Registry::default(), &Theme::builtin("dark").unwrap(), &scene, &ui(worlds), &mut canvas);
    let text: String = (0..40).map(|y| canvas.row_text(y)).collect::<Vec<_>>().join("\n");
    assert!(text.contains("environment") && text.contains("who is doing what"), "{text}");
}

#[test]
fn unknown_components_and_bad_props_fail_at_load() {
    let errs = setup(r#"View(layout: C(name: "Wrold"))"#).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("unknown component 'Wrold'")), "{errs:?}");
    let errs = setup(r#"View(layout: C(name: "World", props: (projection: (dim: "4D"))))"#).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("dim '4D'")), "{errs:?}");
    let errs = setup(r#"View(components: { "A": C(name: "B"), "B": C(name: "A") }, layout: C(name: "A"))"#).err().expect("must fail");
    assert!(errs.iter().any(|e| e.contains("contains itself")), "{errs:?}");
}

#[test]
fn composites_expand_and_each_world_gets_its_own_projection() {
    let src = r#"View(
        components: { "Pair": Cols([ (Fill, C(name: "World", props: (projection: (dim: "2D")))),
                                     (Fill, C(name: "World", props: (projection: (dim: "3D", yaw: 90)))) ]) },
        layout: Rows([ (Fill, C(name: "Pair")), (Fill, C(name: "Pair")) ]))"#;
    let (_, _, worlds) = setup(src).expect("valid");
    assert_eq!(worlds.len(), 4, "two composites, two worlds each");
}

#[test]
fn a_class_overrides_theme_tokens() {
    let theme = Theme::builtin("dark").unwrap();
    let base = sim_render::Style::new(&theme);
    let warn = base.with_class(&theme, Some("warning"));
    assert_ne!(base.color("text"), warn.color("text"));
    assert_eq!(base.color("bg"), warn.color("bg"), "only the class's tokens change");
    assert_eq!(base.color("no-such-token"), sim_render::Rgb(255, 0, 255), "missing tokens are visible");
}
