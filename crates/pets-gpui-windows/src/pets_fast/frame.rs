// Adapted from gpui-fast (Longbridge), Apache-2.0: gpui-fast-windows 0.1.7, src/fast/frame.rs.
// Pets patch: without layer tiles and composition (`Target`/`PathTargets` dropped, the frame draws into the
// window's render target only); path textures are our lazy ones, created here on the first frame with paths.
//! A frame's batches drawn with state set only when it changes.
//!
//! Upstream's `DirectXRenderer::render` draws each batch through its
//! `PipelineState`, which binds the whole pipeline, the sampler and the
//! texture, and maps the batch constant buffer, for every batch; each path
//! batch also collects its vertices and sprites into new `Vec`s. This draws
//! the same batches in the same order with the same draws, but binds through
//! [`DrawState`], which skips what is already bound, and collects path
//! vertices and sprites into buffers kept from frame to frame.
//!
//! Upstream's loop still runs while a graphics debugger such as PIX captures
//! the frame, so that each batch keeps its labeled event.

use std::slice;

use anyhow::{Context as _, Result};
use gpui::{Path, PrimitiveBatch, ScaledPixels, Scene};
use windows::Win32::Graphics::{
    Direct3D::{D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST, D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP},
    Direct3D11::{
        ID3D11Buffer, ID3D11Device, ID3D11DeviceContext, ID3D11RenderTargetView,
        ID3D11SamplerState, ID3D11ShaderResourceView,
    },
};

use crate::DirectXRenderer;
use crate::directx_renderer::{
    PathRasterizationSprite, PathSprite, PathTextures, PipelineState, RENDER_TARGET_FORMAT,
};
use crate::pets_fast::draw_state::DrawState;
use crate::pets_fast::globals::UploadedGlobals;

/// The renderer's gpui-fast state, kept from frame to frame.
#[derive(Default)]
pub(crate) struct FrameState {
    pub(crate) globals: UploadedGlobals,
    pub(crate) state: DrawState,
    path_vertices: Vec<PathRasterizationSprite>,
    path_sprites: Vec<PathSprite>,
    /// Pets patch, tests only: draw through upstream's loop, the reference the tests compare against.
    #[cfg(test)]
    pub(crate) upstream_loop: bool,
}

/// Draws `scene`'s batches in place of `DirectXRenderer::render`'s loop, once
/// the scene's instances are uploaded. Returns `false`, having drawn nothing,
/// when a graphics debugger is capturing and upstream's labeled loop should
/// run instead.
pub(crate) fn draw_batches(renderer: &mut DirectXRenderer, scene: &Scene) -> Result<bool> {
    let devices = renderer.devices.as_ref().context("devices missing")?;
    let capturing = devices
        .annotation
        .as_ref()
        .is_some_and(|annotation| unsafe { annotation.GetStatus().as_bool() });
    #[cfg(test)]
    let capturing = capturing || renderer.fast_frame.upstream_loop;
    if capturing {
        return Ok(false);
    }
    let resources = renderer.resources.as_mut().context("resources missing")?;
    // Pets patch: our lazy path textures, as in `draw_paths_to_intermediate`.
    if !scene.paths.is_empty() && resources.path_textures.is_none() {
        log::debug!("path textures {}x{}", renderer.width, renderer.height);
        resources.path_textures = Some(PathTextures::new(
            &devices.device,
            renderer.width,
            renderer.height,
        )?);
    }
    let resources = &*resources;
    let batch_params = renderer
        .globals
        .batch_params_buffer
        .as_ref()
        .context("batch params buffer missing")?;
    let FrameState {
        state,
        path_vertices,
        path_sprites,
        ..
    } = &mut renderer.fast_frame;
    let pipelines = &mut renderer.pipelines;
    let device_context = &devices.device_context;
    state.forget();

    let mut draw = Draw {
        device_context,
        state,
        batch_params,
        sampler: &renderer.globals.sampler,
    };
    for batch in scene.batches() {
        match batch {
            PrimitiveBatch::Shadows(range) => {
                draw.instances(&pipelines.shadow_pipeline, range.start, range.len(), None)
            }
            PrimitiveBatch::Quads(range) => {
                draw.instances(&pipelines.quad_pipeline, range.start, range.len(), None)
            }
            PrimitiveBatch::Paths(range) => {
                let paths = &scene.paths[range];
                if paths.is_empty() {
                    continue;
                }
                let path_textures = resources
                    .path_textures
                    .as_ref()
                    .context("path textures missing")?;
                rasterize_paths(
                    &mut draw,
                    &devices.device,
                    &resources.render_target_view,
                    path_textures,
                    &mut pipelines.path_rasterization_pipeline,
                    path_vertices,
                    paths,
                )
                .and_then(|()| {
                    composite_paths(
                        &mut draw,
                        &devices.device,
                        path_textures,
                        &mut pipelines.path_sprite_pipeline,
                        path_sprites,
                        paths,
                    )
                })
            }
            PrimitiveBatch::Underlines(range) => draw.instances(
                &pipelines.underline_pipeline,
                range.start,
                range.len(),
                None,
            ),
            PrimitiveBatch::MonochromeSprites { texture_id, range } => {
                // The atlas has released the texture; upstream skips the batch.
                let Some([view]) = renderer.atlas.get_texture_view(texture_id) else {
                    continue;
                };
                let pipeline = &pipelines.mono_sprites;
                draw.instances(pipeline, range.start, range.len(), Some(&view))
            }
            PrimitiveBatch::SubpixelSprites { texture_id, range } => {
                // The atlas has released the texture; upstream skips the batch.
                let Some([view]) = renderer.atlas.get_texture_view(texture_id) else {
                    continue;
                };
                let pipeline = &pipelines.subpixel_sprites;
                draw.instances(pipeline, range.start, range.len(), Some(&view))
            }
            PrimitiveBatch::PolychromeSprites { texture_id, range } => {
                // The atlas has released the texture; upstream skips the batch.
                let Some([view]) = renderer.atlas.get_texture_view(texture_id) else {
                    continue;
                };
                let pipeline = &pipelines.poly_sprites;
                draw.instances(pipeline, range.start, range.len(), Some(&view))
            }
            // Upstream draws nothing for surfaces on Windows either.
            PrimitiveBatch::Surfaces(_) => Ok(()),
        }
        .with_context(|| scene_too_large(scene))?;
    }
    Ok(true)
}

/// What every batch's draw needs.
struct Draw<'a> {
    device_context: &'a ID3D11DeviceContext,
    state: &'a mut DrawState,
    batch_params: &'a ID3D11Buffer,
    sampler: &'a Option<ID3D11SamplerState>,
}

impl Draw<'_> {
    /// Upstream's `PipelineState::draw_range` (and `draw_range_with_texture`
    /// when `texture` is given).
    fn instances<T>(
        &mut self,
        pipeline: &PipelineState<T>,
        first_instance: usize,
        instance_count: usize,
        texture: Option<&Option<ID3D11ShaderResourceView>>,
    ) -> Result<()> {
        if instance_count == 0 {
            return Ok(());
        }
        anyhow::ensure!(
            first_instance + instance_count <= pipeline.buffer_size,
            "DirectX instance range exceeds the {} buffer",
            pipeline.label
        );
        let context = self.device_context;
        self.state
            .set_batch_start(context, self.batch_params, first_instance as u32)?;
        self.state
            .set_pipeline(context, pipeline, D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
        if let Some(texture) = texture {
            self.state.set_texture(context, texture, self.sampler);
        }
        unsafe { context.DrawInstanced(4, instance_count as u32, 0, 0) };
        Ok(())
    }
}

/// Upstream's `DirectXRenderer::draw_paths_to_intermediate`.
fn rasterize_paths(
    draw: &mut Draw<'_>,
    device: &ID3D11Device,
    render_target_view: &Option<ID3D11RenderTargetView>,
    targets: &PathTextures,
    pipeline: &mut PipelineState<PathRasterizationSprite>,
    vertices: &mut Vec<PathRasterizationSprite>,
    paths: &[Path<ScaledPixels>],
) -> Result<()> {
    let context = draw.device_context;
    let msaa_view = targets
        .msaa_view
        .as_ref()
        .context("path MSAA view missing")?;
    unsafe {
        context.ClearRenderTargetView(msaa_view, &[0.0; 4]);
        context.OMSetRenderTargets(Some(slice::from_ref(&targets.msaa_view)), None);
    }
    draw.state.forget_views();

    vertices.clear();
    for path in paths {
        let bounds = path.clipped_bounds();
        vertices.extend(path.vertices.iter().map(|v| PathRasterizationSprite {
            xy_position: v.xy_position,
            st_position: v.st_position,
            color: path.color,
            bounds,
        }));
    }
    pipeline.update_buffer(device, context, vertices)?;
    draw.state
        .set_pipeline(context, pipeline, D3D_PRIMITIVE_TOPOLOGY_TRIANGLELIST);
    unsafe {
        context.DrawInstanced(vertices.len() as u32, 1, 0, 0);
        context.ResolveSubresource(
            &targets.texture,
            0,
            &targets.msaa_texture,
            0,
            RENDER_TARGET_FORMAT,
        );
        context.OMSetRenderTargets(Some(slice::from_ref(render_target_view)), None);
    }
    draw.state.forget_views();
    Ok(())
}

/// Upstream's `DirectXRenderer::draw_paths_from_intermediate`.
fn composite_paths(
    draw: &mut Draw<'_>,
    device: &ID3D11Device,
    targets: &PathTextures,
    pipeline: &mut PipelineState<PathSprite>,
    sprites: &mut Vec<PathSprite>,
    paths: &[Path<ScaledPixels>],
) -> Result<()> {
    let (Some(first_path), Some(last_path)) = (paths.first(), paths.last()) else {
        return Ok(());
    };

    // Each pixel must be copied only once, in case of transparent paths.
    // Paths of one draw order are disjoint and copied one by one; a batch
    // mixing draw orders is copied as one rect spanning all of them.
    sprites.clear();
    if last_path.order == first_path.order {
        sprites.extend(paths.iter().map(|path| PathSprite {
            bounds: path.clipped_bounds(),
        }));
    } else {
        let mut bounds = first_path.clipped_bounds();
        for path in &paths[1..] {
            bounds = bounds.union(&path.clipped_bounds());
        }
        sprites.push(PathSprite { bounds });
    }

    let context = draw.device_context;
    pipeline.update_buffer(device, context, sprites)?;
    draw.state
        .set_pipeline(context, pipeline, D3D_PRIMITIVE_TOPOLOGY_TRIANGLESTRIP);
    draw.state.set_texture(context, &targets.srv, draw.sampler);
    unsafe { context.DrawInstanced(4, sprites.len() as u32, 0, 0) };
    Ok(())
}

fn scene_too_large(scene: &Scene) -> String {
    format!(
        "scene too large:\
        {} paths, {} shadows, {} quads, {} underlines, {} mono, {} subpixel, {} poly, {} surfaces",
        scene.paths.len(),
        scene.shadows.len(),
        scene.quads.len(),
        scene.underlines.len(),
        scene.monochrome_sprites.len(),
        scene.subpixel_sprites.len(),
        scene.polychrome_sprites.len(),
        scene.surfaces.len(),
    )
}

// Pets patch: the frame drawn here is bit-for-bit upstream's, across frames, resizes and path batches.
#[cfg(test)]
mod tests {
    use std::borrow::Cow;

    use gpui::{
        AtlasKey, AtlasTile, Background, Bounds, ContentMask, Corners, DevicePixels, Edges, Hsla,
        ImageId, Path, PolychromeSprite, Quad, RenderImageParams, ScaledPixels, Scene, Shadow,
        Underline, WindowBackgroundAppearance, hsla, linear_color_stop, linear_gradient, point, px,
        size,
    };
    use windows::Win32::Foundation::HWND;
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_OVERLAPPEDWINDOW,
    };
    use windows::core::w;

    use crate::DirectXRenderer;
    use crate::directx_devices::DirectXDevices;

    fn sb(x: f32, y: f32, w: f32, h: f32) -> Bounds<ScaledPixels> {
        Bounds::new(
            point(ScaledPixels(x), ScaledPixels(y)),
            size(ScaledPixels(w), ScaledPixels(h)),
        )
    }

    fn mask() -> ContentMask<ScaledPixels> {
        ContentMask {
            bounds: sb(0., 0., 4096., 4096.),
        }
    }

    fn gradient() -> Background {
        linear_gradient(
            90.,
            linear_color_stop(hsla(0.6, 0.5, 0.08, 1.), 0.),
            linear_color_stop(hsla(0.6, 0.5, 0.16, 1.), 1.),
        )
    }

    fn quad(x: f32, y: f32, w: f32, h: f32, background: Background) -> Quad {
        Quad {
            bounds: sb(x, y, w, h),
            content_mask: mask(),
            background,
            border_color: hsla(0., 0., 0., 1.),
            corner_radii: Corners::all(ScaledPixels(6.)),
            border_widths: Edges::all(ScaledPixels(1.)),
            ..Default::default()
        }
    }

    fn triangle(x: f32, y: f32, color: Hsla) -> Path<ScaledPixels> {
        let mut path = Path::new(point(px(x), px(y)));
        path.line_to(point(px(x + 60.), px(y + 10.)));
        path.line_to(point(px(x + 20.), px(y + 50.)));
        path.color = color.into();
        path.content_mask = ContentMask {
            bounds: Bounds::new(point(px(0.), px(0.)), size(px(4096.), px(4096.))),
        };
        path.scale(1.0)
    }

    fn sprite(tile: AtlasTile, x: f32, y: f32) -> PolychromeSprite {
        PolychromeSprite {
            order: 0,
            pad: 0,
            grayscale: false.into(),
            opacity: 1.0,
            bounds: sb(x, y, 32., 32.),
            content_mask: mask(),
            corner_radii: Corners::default(),
            tile,
        }
    }

    fn image(renderer: &DirectXRenderer, id: usize, side: i32) -> AtlasTile {
        let mut bytes = Vec::new();
        for y in 0..side {
            for x in 0..side {
                bytes.extend_from_slice(&[(x * 7) as u8, (y * 5) as u8, (id * 60) as u8, 255]);
            }
        }
        renderer
            .sprite_atlas()
            .get_or_insert_with(
                AtlasKey::Image(RenderImageParams {
                    image_id: ImageId(id),
                    frame_index: 0,
                }),
                &mut || {
                    Ok(Some((
                        size(DevicePixels(side), DevicePixels(side)),
                        Cow::Borrowed(bytes.as_slice()),
                    )))
                },
            )
            .expect("atlas insert")
            .expect("tile")
    }

    /// Every batch kind the frame draws, interleaved so pipelines, textures and path passes alternate;
    /// `dy` scrolls the content.
    fn scene(renderer: &DirectXRenderer, dy: f32) -> Scene {
        // The second image does not fit the first atlas page: two sprite textures.
        let (small, big) = (image(renderer, 1, 32), image(renderer, 2, 1100));
        let mut scene = Scene::default();
        scene.insert_primitive(Shadow {
            order: 0,
            blur_radius: ScaledPixels(8.),
            bounds: sb(10., 10. + dy, 120., 60.),
            corner_radii: Corners::all(ScaledPixels(6.)),
            content_mask: mask(),
            color: hsla(0., 0., 0., 0.4),
            element_bounds: sb(10., 10. + dy, 120., 60.),
            element_corner_radii: Corners::all(ScaledPixels(6.)),
            inset: 0,
            pad: 0,
        });
        scene.insert_primitive(quad(10., 10. + dy, 180., 120., gradient()));
        scene.insert_primitive(triangle(20., 20. + dy, hsla(0., 1., 0.5, 0.6)));
        scene.insert_primitive(sprite(small, 40., 30. + dy));
        scene.insert_primitive(quad(
            30.,
            40. + dy,
            60.,
            40.,
            hsla(0.3, 0.8, 0.5, 0.7).into(),
        ));
        scene.insert_primitive(triangle(50., 50. + dy, hsla(0.6, 1., 0.5, 0.6)));
        scene.insert_primitive(triangle(120., 20. + dy, hsla(0.1, 1., 0.5, 1.)));
        scene.insert_primitive(sprite(big, 60., 60. + dy));
        scene.insert_primitive(sprite(small, 100., 70. + dy));
        scene.insert_primitive(Underline {
            order: 0,
            pad: 0,
            bounds: sb(20., 100. + dy, 140., 4.),
            content_mask: mask(),
            color: hsla(0., 0., 0.1, 1.),
            thickness: ScaledPixels(2.),
            wavy: true.into(),
        });
        scene.insert_primitive(triangle(70., 80. + dy, hsla(0.8, 1., 0.5, 0.5)));
        scene.finish();
        scene
    }

    fn window() -> anyhow::Result<HWND> {
        Ok(unsafe {
            CreateWindowExW(
                WINDOW_EX_STYLE(0),
                w!("STATIC"),
                w!("pets_fast test"),
                WS_OVERLAPPEDWINDOW,
                0,
                0,
                200,
                200,
                None,
                None,
                None,
                None,
            )
        }?)
    }

    fn render(r: &mut DirectXRenderer, scene: &Scene, upstream: bool) -> image::RgbaImage {
        r.fast_frame.upstream_loop = upstream;
        r.render_to_image(scene, WindowBackgroundAppearance::Opaque)
            .expect("render")
    }

    #[test]
    fn pets_fast_frame_matches_upstream_loop() -> anyhow::Result<()> {
        let hwnd = window()?;
        let devices = DirectXDevices::new()?;
        let mut r = DirectXRenderer::new(hwnd, &devices, true)?;
        r.resize(size(DevicePixels(200), DevicePixels(160)))?;
        let (a, b) = (scene(&r, 0.), scene(&r, -13.));
        let mut empty = Scene::default();
        empty.finish();

        // Fast frames in a row (state carried over), switched with upstream frames, both ways.
        for (step, scene) in [&a, &b, &a, &empty, &b].into_iter().enumerate() {
            let fast = render(&mut r, scene, false);
            let upstream = render(&mut r, scene, true);
            let again = render(&mut r, scene, false);
            assert!(fast == upstream, "frame {step}: fast differs from upstream");
            assert!(
                fast == again,
                "frame {step}: fast differs after an upstream frame"
            );
        }
        // A resize changes the global parameters and drops the lazy path textures.
        r.resize(size(DevicePixels(240), DevicePixels(180)))?;
        let fast = render(&mut r, &a, false);
        let upstream = render(&mut r, &a, true);
        assert_eq!((fast.width(), fast.height()), (240, 180));
        assert!(fast == upstream, "after resize: fast differs from upstream");

        drop(r);
        unsafe { DestroyWindow(hwnd) }?;
        Ok(())
    }

    /// The gradient dither is seeded relative to the quad: scrolled by whole pixels, the quad's pixels
    /// are the same (upstream seeded it by the screen position, and the pattern shimmered).
    #[test]
    fn pets_fast_gradient_dither_scrolls_with_quad() -> anyhow::Result<()> {
        let hwnd = window()?;
        let devices = DirectXDevices::new()?;
        let mut r = DirectXRenderer::new(hwnd, &devices, true)?;
        r.resize(size(DevicePixels(200), DevicePixels(160)))?;
        let at = |dy: f32| {
            let mut scene = Scene::default();
            scene.insert_primitive(Quad {
                corner_radii: Corners::default(),
                border_widths: Edges::default(),
                ..quad(20., 20. + dy, 160., 100., gradient())
            });
            scene.finish();
            scene
        };
        let (top, scrolled) = (
            render(&mut r, &at(0.), false),
            render(&mut r, &at(17.), false),
        );
        for y in 22..118 {
            for x in 22..178 {
                assert_eq!(
                    top.get_pixel(x, y),
                    scrolled.get_pixel(x, y + 17),
                    "pixel ({x}, {y})"
                );
            }
        }

        drop(r);
        unsafe { DestroyWindow(hwnd) }?;
        Ok(())
    }
}
