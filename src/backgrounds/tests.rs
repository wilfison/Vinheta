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
