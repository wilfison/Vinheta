use super::*;
use gtk::gdk_pixbuf::Colorspace;
use std::fs;
use std::sync::atomic::{AtomicU32, Ordering};

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let unique = format!(
            "vinheta-backgrounds-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        );
        let dir = std::env::temp_dir().join(unique);
        fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }

    /// An image of one color written as `name`, in the format of its
    /// extension.
    fn image(&self, name: &str, width: i32, height: i32, alpha: bool) -> PathBuf {
        let pixbuf = Pixbuf::new(Colorspace::Rgb, alpha, 8, width, height).unwrap();
        pixbuf.fill(0x3366ccff);
        let path = self.0.join(name);
        let kind = if name.ends_with(".png") {
            "png"
        } else {
            "jpeg"
        };
        pixbuf.savev(&path, kind, &[]).unwrap();
        path
    }

    fn stored(&self) -> PathBuf {
        self.0.join("backgrounds")
    }

    fn size(&self, name: &str) -> (i32, i32) {
        let pixbuf = Pixbuf::from_file(self.stored().join(name)).unwrap();
        (pixbuf.width(), pixbuf.height())
    }

    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(self.stored())
            .unwrap()
            .map(|entry| entry.unwrap().file_name().into_string().unwrap())
            .collect();
        names.sort();
        names
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn images_are_told_by_their_extension() {
    for name in ["a.png", "a.jpg", "a.jpeg", "A.PNG"] {
        assert!(is_image(Path::new(name)), "{name}");
    }
    for name in ["a.wav", "a.txt", "png", "a"] {
        assert!(!is_image(Path::new(name)), "{name}");
    }
}

#[test]
fn split_keeps_the_order() {
    let paths = ["a.wav", "b.png", "c", "d.jpg"].map(PathBuf::from).to_vec();
    let (images, others) = split(paths);
    assert_eq!(images, ["b.png", "d.jpg"].map(PathBuf::from));
    assert_eq!(others, ["a.wav", "c"].map(PathBuf::from));
}

#[test]
fn a_large_image_is_reduced_to_a_jpeg() {
    let scratch = Scratch::new();
    let source = scratch.image("photo.jpg", 4000, 3000, false);
    let name = store(&source, &scratch.stored()).unwrap();
    assert!(name.ends_with(".jpg"), "{name}");
    assert_eq!(name.len(), 16 + ".jpg".len());
    assert_eq!(scratch.size(&name), (512, 384));
    assert!(source.is_file());
}

#[test]
fn an_image_with_alpha_is_a_png() {
    let scratch = Scratch::new();
    let source = scratch.image("logo.png", 600, 1200, true);
    let name = store(&source, &scratch.stored()).unwrap();
    assert!(name.ends_with(".png"), "{name}");
    assert_eq!(scratch.size(&name), (256, 512));
}

#[test]
fn a_small_image_is_not_enlarged() {
    let scratch = Scratch::new();
    let source = scratch.image("small.png", 100, 50, false);
    let name = store(&source, &scratch.stored()).unwrap();
    assert_eq!(scratch.size(&name), (100, 50));
}

#[test]
fn a_photo_taken_sideways_comes_out_upright() {
    let scratch = Scratch::new();
    let source = scratch.0.join("sideways.jpg");
    fs::write(&source, include_bytes!("sideways.jpg")).unwrap();
    let name = store(&source, &scratch.stored()).unwrap();
    assert_eq!(scratch.size(&name), (4, 8));
}

#[test]
fn what_is_not_an_image_is_an_error() {
    let scratch = Scratch::new();
    let text = scratch.0.join("notes.png");
    fs::write(&text, "this is not an image").unwrap();
    assert_eq!(store(&text, &scratch.stored()), Err(Error::NotAnImage));
    let missing = scratch.0.join("missing.png");
    assert_eq!(store(&missing, &scratch.stored()), Err(Error::NotAnImage));
    assert!(!scratch.stored().exists());
}

#[test]
fn the_same_image_is_stored_once() {
    let scratch = Scratch::new();
    let source = scratch.image("photo.jpg", 800, 600, false);
    let first = store(&source, &scratch.stored()).unwrap();
    let copy = scratch.0.join("copy.jpg");
    fs::copy(&source, &copy).unwrap();
    let second = store(&copy, &scratch.stored()).unwrap();
    assert_eq!(first, second);
    assert_eq!(scratch.files(), [first]);
}

#[test]
fn sweep_removes_what_no_pad_uses() {
    let scratch = Scratch::new();
    let dir = scratch.stored();
    fs::create_dir_all(dir.join("folder")).unwrap();
    for name in ["kept.jpg", "gone.jpg", ".gone.jpg.part"] {
        fs::write(dir.join(name), b"").unwrap();
    }
    let referenced = BTreeSet::from(["kept.jpg".to_owned(), "elsewhere.png".to_owned()]);
    assert_eq!(sweep(&dir, &referenced), 2);
    assert_eq!(scratch.files(), ["folder", "kept.jpg"]);
}

#[test]
fn sweep_of_a_missing_directory_does_nothing() {
    let scratch = Scratch::new();
    assert_eq!(sweep(&scratch.stored(), &BTreeSet::new()), 0);
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() < 1e-3
}

fn close_crop(crop: Crop, expected: (f64, f64, f64, f64)) -> bool {
    close(crop.x, expected.0)
        && close(crop.y, expected.1)
        && close(crop.width, expected.2)
        && close(crop.height, expected.3)
}

#[test]
fn fit_is_the_largest_centered_crop_of_the_aspect() {
    let wide = Crop::fit((512.0, 384.0), PAD_ASPECT);
    assert!(close_crop(wide, (0.0, 0.0556, 1.0, 0.8889)), "{wide:?}");
    let tall = Crop::fit((384.0, 512.0), PAD_ASPECT);
    assert!(close_crop(tall, (0.0, 0.25, 1.0, 0.5)), "{tall:?}");
    let square = Crop::fit((300.0, 300.0), 1.0);
    assert!(close_crop(square, (0.0, 0.0, 1.0, 1.0)), "{square:?}");
    let panorama = Crop::fit((900.0, 300.0), PAD_ASPECT);
    assert!(close_crop(panorama, (0.25, 0.0, 0.5, 1.0)), "{panorama:?}");
}

#[test]
fn zoomed_keeps_the_center_and_the_limits() {
    let image = (512.0, 384.0);
    let fit = Crop::fit(image, PAD_ASPECT);
    let two = fit.zoomed(image, PAD_ASPECT, 2.0);
    assert!(close(two.width, fit.width / 2.0) && close(two.height, fit.height / 2.0));
    assert!(close(two.x + two.width / 2.0, 0.5) && close(two.y + two.height / 2.0, 0.5));
    let four = fit.zoomed(image, PAD_ASPECT, 4.0);
    assert!(close(four.width, fit.width / MAX_ZOOM));
    let below = fit.zoomed(image, PAD_ASPECT, 0.5);
    assert!(close_crop(below, (fit.x, fit.y, fit.width, fit.height)));
    // A crop at a corner, zoomed out, is pushed back inside the picture.
    let corner = Crop {
        x: 0.75,
        y: 0.75,
        width: 0.25,
        height: 0.25,
    };
    let out = corner.zoomed(image, PAD_ASPECT, 1.0);
    assert!(out.is_valid(), "{out:?}");
    assert!(close(out.x + out.width, 1.0) && close(out.y + out.height, 1.0));
}

#[test]
fn zoom_is_read_back() {
    let image = (512.0, 384.0);
    let fit = Crop::fit(image, PAD_ASPECT);
    assert!(close(fit.zoom(image, PAD_ASPECT), 1.0));
    assert!(close(
        fit.zoomed(image, PAD_ASPECT, 2.0).zoom(image, PAD_ASPECT),
        2.0
    ));
}

#[test]
fn moved_stops_at_the_edges() {
    let crop = Crop {
        x: 0.25,
        y: 0.25,
        width: 0.5,
        height: 0.5,
    };
    assert!(close_crop(crop.moved(0.1, -0.1), (0.35, 0.15, 0.5, 0.5)));
    assert!(close_crop(crop.moved(2.0, -2.0), (0.5, 0.0, 0.5, 0.5)));
}

#[test]
fn cover_without_a_crop_is_what_the_pad_drew() {
    let (x, y, width, height) = cover((512.0, 384.0), None, (144.0, 96.0));
    assert!(close(x, 0.0) && close(y, -6.0) && close(width, 144.0) && close(height, 108.0));
}

#[test]
fn cover_fills_the_target_with_the_crop() {
    let right_half = Crop {
        x: 0.5,
        y: 0.0,
        width: 0.5,
        height: 1.0,
    };
    let (x, _, width, _) = cover((512.0, 384.0), Some(right_half), (144.0, 96.0));
    assert!(close(x, -144.0) && close(width, 288.0), "{x} {width}");

    let image = (512.0, 384.0);
    let crop = Crop::fit(image, PAD_ASPECT).zoomed(image, PAD_ASPECT, 2.0);
    let crop = crop.moved(0.2, 0.1);
    let (x, y, width, height) = cover(image, Some(crop), (144.0, 96.0));
    // The crop touches all four sides of the target.
    assert!(close(x + crop.x * width, 0.0) && close(y + crop.y * height, 0.0));
    assert!(close(crop.width * width, 144.0) && close(crop.height * height, 96.0));
}

#[test]
fn dragged_moves_the_crop_against_the_pointer() {
    let image = (512.0, 384.0);
    let frame = (294.0, 196.0);
    let crop = Crop::fit(image, PAD_ASPECT).zoomed(image, PAD_ASPECT, 2.0);
    // 80 pixels of the frame to the left: the crop goes right by 80 / 294
    // of its width.
    let left = crop.dragged(image, frame, -80.0, 0.0);
    assert!(
        close(left.x, crop.x + 80.0 / 294.0 * crop.width),
        "{left:?}"
    );
    assert!(close(left.y, crop.y));
    let down = crop.dragged(image, frame, 0.0, 49.0);
    assert!(
        close(down.y, crop.y - 49.0 / 196.0 * crop.height),
        "{down:?}"
    );
    let far = crop.dragged(image, frame, -10_000.0, 10_000.0);
    assert!(
        close(far.x + far.width, 1.0) && close(far.y, 0.0),
        "{far:?}"
    );
}

#[test]
fn nudged_moves_by_its_own_size() {
    let crop = Crop {
        x: 0.25,
        y: 0.25,
        width: 0.5,
        height: 0.4,
    };
    assert!(close_crop(crop.nudged(0.02, -0.1), (0.26, 0.21, 0.5, 0.4)));
    assert!(close_crop(crop.nudged(1.0, 1.0), (0.5, 0.6, 0.5, 0.4)));
}
