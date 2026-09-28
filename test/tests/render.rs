use sim_core::{Engine, Loaded};
use sim_render::{Assets, Canvas, Registry, Scene, Theme, Ui, View};
use sim_rules::Game;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../games/colony");

type Setup = (Engine<sim_core::Running, Game>, View, Vec<sim_render::Projection>);

fn setup(view_src: &str) -> Result<Setup, Vec<String>> {
    let (world, game) = Game::load(std::path::Path::new(DIR), None).map_err(|e| vec![e])?;
    let engine = Engine::<Loaded, _>::new(world, game).validate()?.start();
    let mut view: View = ron::from_str(view_src).map_err(|e| vec![e.to_string()])?;
    let worlds = view.resolve(&Registry::default(), engine.world().depth)?;
    Ok((engine, view, worlds))
}

fn ui(worlds: Vec<sim_render::Projection>) -> Ui {
    Ui::new(worlds)
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

// --- 2.5D layers ---

const LAYERS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../games/colony3d");

fn layered_setup() -> (Engine<sim_core::Running, Game>, View, Vec<sim_render::Projection>, Assets) {
    let (world, game) = Game::load(std::path::Path::new(LAYERS), None).expect("loads");
    let engine = Engine::<Loaded, _>::new(world, game).validate().expect("valid").start();
    let src = std::fs::read_to_string(format!("{LAYERS}/views/layers.ron")).unwrap();
    let mut view: View = ron::from_str(&src).expect("parses");
    let worlds = view.resolve(&Registry::default(), engine.world().depth).expect("resolves");
    let assets: Assets = ron::from_str(&std::fs::read_to_string(format!("{LAYERS}/../../assets/ants.ron")).unwrap()).unwrap();
    (engine, view, worlds, assets)
}

fn stats(ui: &Ui) -> sim_render::layered::Stats {
    match &ui.worlds[0] {
        sim_render::Projection::Layers(l) => l.stats(),
        _ => panic!("the demo's first world is 2.5D"),
    }
}

#[test]
fn layers_redraw_lazily() {
    let (mut engine, view, worlds, assets) = layered_setup();
    let mut ui = ui(worlds);
    let theme = Theme::builtin("dark").unwrap();
    let mut canvas = Canvas::new(120, 40);
    let mut draw = |engine: &Engine<sim_core::Running, Game>, ui: &Ui| {
        let scene = Scene { world: engine.world(), game: engine.rules(), assets: &assets };
        view.draw(&Registry::default(), &theme, &scene, ui, &mut canvas);
    };
    draw(&engine, &ui);
    let first = stats(&ui);
    assert_eq!((first.redrawn, first.visible, first.reused), (3, 3, false), "first frame: the 3 visible layers only (of 6)");
    draw(&engine, &ui);
    assert!(stats(&ui).reused, "nothing changed: the last composite is reused");
    engine.tick();
    ui.tick = engine.world().tick;
    draw(&engine, &ui);
    let s = stats(&ui);
    assert!(!s.reused && s.redrawn < s.visible, "a tick redraws only the layers that changed: {s:?}");
}

#[test]
fn a_click_follows_the_perspective_states() {
    let (_, _, mut worlds, _) = layered_setup();
    let sim_render::Projection::Layers(l) = &mut worlds[0] else { panic!("2.5D") };
    let names: Vec<String> = (0..5)
        .map(|_| {
            let n = l.perspective().name.clone();
            l.click();
            n
        })
        .collect();
    assert_eq!(names, ["surface", "granary", "deep", "stack", "surface"]);
}

#[test]
fn bad_perspectives_fail_at_load() {
    let (engine, ..) = layered_setup();
    let depth = engine.world().depth;
    for (src, want) in [
        (r#"(name: "a", order: [9])"#, "level 9 does not exist"),
        (r#"(name: "a", order: [0, 1], focus: 3)"#, "focus 3 is not in its order"),
        (r#"(name: "a", order: [0], click: "b")"#, "unknown perspective 'b'"),
    ] {
        let view = format!(r#"View(layout: C(name: "World", props: (projection: (dim: "2.5D", perspectives: [{src}]))))"#);
        let mut v: View = ron::from_str(&view).unwrap();
        let errs = v.resolve(&Registry::default(), depth).expect_err("must fail");
        assert!(errs.iter().any(|e| e.contains(want)), "want '{want}' in {errs:?}");
    }
}

#[test]
fn the_diorama_paints_pixels_in_both_modes() {
    let (world, game) = Game::load(std::path::Path::new(LAYERS), None).expect("loads");
    let engine = Engine::<Loaded, _>::new(world, game).validate().expect("valid").start();
    let src = std::fs::read_to_string(format!("{LAYERS}/views/diorama.ron")).unwrap();
    let mut view: View = ron::from_str(&src).expect("parses");
    let worlds = view.resolve(&Registry::default(), engine.world().depth).expect("resolves");
    let dir = format!("{LAYERS}/../../assets");
    let load = |n: &str| -> Assets { ron::from_str(&std::fs::read_to_string(format!("{dir}/{n}.ron")).unwrap()).unwrap() };
    let assets = load("ants").merged(load("ants_pixel"));
    assert!(assets.sprites.contains_key("ant_carry") && assets.palette.contains_key(&'k'));
    let scene = Scene { world: engine.world(), game: engine.rules(), assets: &assets };
    let theme = Theme::builtin("dark").unwrap();
    let mut ui = Ui::new(worlds);
    ui.graphics = true;
    let mut canvas = Canvas::new(120, 40);
    view.draw(&Registry::default(), &theme, &scene, &ui, &mut canvas);
    let images = ui.images.borrow();
    assert_eq!(images.len(), 1, "true-pixel mode hands the host one image");
    assert!(images[0].1.w > 100, "{}x{}", images[0].1.w, images[0].1.h);
    drop(images);
    ui.graphics = false;
    ui.images.borrow_mut().clear();
    let mut canvas = Canvas::new(120, 40);
    view.draw(&Registry::default(), &theme, &scene, &ui, &mut canvas);
    assert!(ui.images.borrow().is_empty(), "half-block mode draws into cells");
    let blocks = (0..40).map(|y| canvas.row_text(y)).filter(|r| r.contains('▀')).count();
    assert!(blocks > 20, "the picture is made of half-blocks ({blocks} rows)");
}

#[test]
fn generated_art_loads_from_the_landscape_pack() {
    let (world, game) = Game::load(std::path::Path::new(LAYERS), None).expect("loads");
    let engine = Engine::<Loaded, _>::new(world, game).validate().expect("valid").start();
    let src = std::fs::read_to_string(format!("{LAYERS}/views/generated.ron")).unwrap();
    let mut view: View = ron::from_str(&src).expect("parses");
    let worlds = view.resolve(&Registry::default(), engine.world().depth).expect("resolves");
    let dir = std::path::PathBuf::from(format!("{LAYERS}/../../assets"));
    let mut assets = Assets::default();
    for n in ["ants", "ants_pixel", "landscape"] {
        let mut pack: Assets = ron::from_str(&std::fs::read_to_string(dir.join(format!("{n}.ron"))).unwrap()).unwrap();
        pack.load_images(&dir).expect("images load");
        assets = assets.merged(pack);
    }
    assert_eq!(assets.loaded["soil"].w, 32);
    let scene = Scene { world: engine.world(), game: engine.rules(), assets: &assets };
    let mut ui = Ui::new(worlds);
    ui.graphics = true;
    let mut canvas = Canvas::new(120, 40);
    view.draw(&Registry::default(), &Theme::builtin("dark").unwrap(), &scene, &ui, &mut canvas);
    let text: String = (0..40).map(|y| canvas.row_text(y)).collect();
    assert!(!text.contains("Diorama:"), "no component error: {text}");
    assert_eq!(ui.images.borrow().len(), 1);
}
