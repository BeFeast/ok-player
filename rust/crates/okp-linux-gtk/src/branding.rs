use super::*;
use okp_core::branding::{
    CANONICAL_FULL_MARK, FullMarkGeometry, MARK_O_CENTER, MARK_O_RADIUS, MARK_STEM_HEIGHT,
    MARK_STEM_RADIUS, MARK_STEM_Y, MARK_TRIANGLE_APEX_Y, MARK_TRIANGLE_X, MARK_VIEWBOX_HEIGHT,
    MARK_VIEWBOX_WIDTH, full_mark_for_icon_size,
};

pub(crate) fn launcher_brand_tile(size: i32, css_class: &str) -> gtk::Box {
    let texture = launcher_brand_texture(size);
    let picture = gtk::Picture::for_paintable(&texture);
    picture.set_size_request(size, size);
    picture.set_can_shrink(false);
    picture.set_can_target(false);

    let tile = gtk::Box::new(gtk::Orientation::Horizontal, 0);
    tile.add_css_class("okp-brand-tile");
    tile.add_css_class(css_class);
    tile.set_size_request(size, size);
    tile.set_halign(gtk::Align::Center);
    tile.set_valign(gtk::Align::Center);
    tile.set_can_target(false);
    tile.append(&picture);
    tile
}

fn launcher_brand_texture(size: i32) -> gdk::MemoryTexture {
    let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, size, size)
        .expect("brand tile surface should be allocated");
    let cr = cairo::Context::new(&surface).expect("brand tile context should be created");
    draw_launcher_brand_tile(&cr, size, size, size.max(0) as u32);
    surface.flush();
    drop(cr);

    let stride = surface.stride() as usize;
    let bytes = {
        let data = surface
            .data()
            .expect("brand tile pixels should be readable");
        glib::Bytes::from_owned(data.to_vec())
    };
    gdk::MemoryTexture::new(size, size, launcher_texture_format(), &bytes, stride)
}

const fn launcher_texture_format() -> gdk::MemoryFormat {
    if cfg!(target_endian = "little") {
        gdk::MemoryFormat::B8g8r8a8Premultiplied
    } else {
        gdk::MemoryFormat::A8r8g8b8Premultiplied
    }
}

fn draw_launcher_brand_tile(cr: &cairo::Context, width: i32, height: i32, icon_size: u32) {
    let width = f64::from(width);
    let height = f64::from(height);
    let radius = f64::from(icon_size) * 11.0 / 48.0;

    let _ = cr.save();
    rounded_rect(cr, 0.0, 0.0, width, height, radius);
    cr.clip();
    let gradient = cairo::LinearGradient::new(width * 0.2113, 0.0, width * 0.7887, height);
    gradient.add_color_stop_rgb(0.0, 21.0 / 255.0, 168.0 / 255.0, 157.0 / 255.0);
    gradient.add_color_stop_rgb(1.0, 10.0 / 255.0, 101.0 / 255.0, 95.0 / 255.0);
    let _ = cr.set_source(&gradient);
    let _ = cr.paint();
    let _ = cr.restore();

    let geometry = full_mark_for_icon_size(icon_size);
    draw_full_mark(
        cr,
        width as i32,
        height as i32,
        width * 2.0 / 3.0,
        height * 17.0 / 48.0,
        geometry,
        (1.0, 1.0, 1.0, 1.0),
    );
}

pub(crate) fn canonical_brand_mark(width: i32, height: i32, css_class: &str) -> gtk::DrawingArea {
    let area = gtk::DrawingArea::new();
    area.add_css_class(css_class);
    area.set_content_width(width);
    area.set_content_height(height);
    area.set_size_request(width, height);
    area.set_can_target(false);
    area.set_draw_func(move |area, cr, actual_width, actual_height| {
        let color = area.color();
        draw_full_mark(
            cr,
            actual_width,
            actual_height,
            f64::from(actual_width),
            f64::from(actual_height),
            CANONICAL_FULL_MARK,
            (
                f64::from(color.red()),
                f64::from(color.green()),
                f64::from(color.blue()),
                f64::from(color.alpha()),
            ),
        );
    });
    area
}

fn draw_full_mark(
    cr: &cairo::Context,
    area_width: i32,
    area_height: i32,
    viewport_width: f64,
    viewport_height: f64,
    geometry: FullMarkGeometry,
    color: (f64, f64, f64, f64),
) {
    let scale = f64::min(
        viewport_width / MARK_VIEWBOX_WIDTH,
        viewport_height / MARK_VIEWBOX_HEIGHT,
    );
    let rendered_width = MARK_VIEWBOX_WIDTH * scale;
    let rendered_height = MARK_VIEWBOX_HEIGHT * scale;
    let x = (f64::from(area_width) - rendered_width) / 2.0;
    let y = (f64::from(area_height) - rendered_height) / 2.0;

    let _ = cr.save();
    cr.translate(x, y);
    cr.scale(scale, scale);
    cr.set_source_rgba(color.0, color.1, color.2, color.3);

    cr.set_line_width(geometry.o_stroke);
    cr.arc(
        MARK_O_CENTER.0,
        MARK_O_CENTER.1,
        MARK_O_RADIUS,
        0.0,
        std::f64::consts::TAU,
    );
    let _ = cr.stroke();

    rounded_rect(
        cr,
        geometry.stem_x,
        MARK_STEM_Y,
        geometry.stem_width,
        MARK_STEM_HEIGHT,
        MARK_STEM_RADIUS,
    );
    let _ = cr.fill();

    cr.move_to(MARK_TRIANGLE_X, geometry.triangle_top);
    cr.line_to(MARK_TRIANGLE_X, geometry.triangle_bottom);
    cr.line_to(geometry.triangle_apex_x, MARK_TRIANGLE_APEX_Y);
    cr.close_path();
    cr.set_line_width(geometry.triangle_stroke);
    cr.set_line_join(cairo::LineJoin::Round);
    let _ = cr.fill_preserve();
    let _ = cr.stroke();
    let _ = cr.restore();
}

fn rounded_rect(cr: &cairo::Context, x: f64, y: f64, width: f64, height: f64, radius: f64) {
    let right = x + width;
    let bottom = y + height;
    cr.new_sub_path();
    cr.arc(
        right - radius,
        y + radius,
        radius,
        -std::f64::consts::FRAC_PI_2,
        0.0,
    );
    cr.arc(
        right - radius,
        bottom - radius,
        radius,
        0.0,
        std::f64::consts::FRAC_PI_2,
    );
    cr.arc(
        x + radius,
        bottom - radius,
        radius,
        std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
    );
    cr.arc(
        x + radius,
        y + radius,
        radius,
        std::f64::consts::PI,
        std::f64::consts::PI * 1.5,
    );
    cr.close_path();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn launcher_tile_paints_the_white_mark_with_its_own_gradient() {
        let mut surface = cairo::ImageSurface::create(cairo::Format::ARgb32, 48, 48)
            .expect("test image surface should be created");
        let cr = cairo::Context::new(&surface).expect("test Cairo context should be created");

        draw_launcher_brand_tile(&cr, 48, 48, 48);
        surface.flush();
        drop(cr);

        let stride = surface.stride() as usize;
        let data = surface
            .data()
            .expect("rendered launcher tile pixels should be readable");
        let mut white_pixels = 0;
        for y in 0..48usize {
            let row = &data[y * stride..y * stride + 48 * 4];
            white_pixels += row
                .chunks_exact(4)
                .filter(|pixel| pixel[0] >= 230 && pixel[1] >= 230 && pixel[2] >= 230)
                .count();
        }

        assert!(
            white_pixels >= 150,
            "launcher tile should contain a visible white mark, found {white_pixels} pixels"
        );
    }

    #[test]
    fn launcher_tile_texture_uses_the_cairo_native_pixel_layout() {
        assert_eq!(
            if cfg!(target_endian = "little") {
                gdk::MemoryFormat::B8g8r8a8Premultiplied
            } else {
                gdk::MemoryFormat::A8r8g8b8Premultiplied
            },
            launcher_texture_format()
        );
    }
}
