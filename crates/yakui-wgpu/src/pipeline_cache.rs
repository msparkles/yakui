pub struct PipelineCache {
    layout: wgpu::PipelineLayout,
    current: Option<Cached>,
}

struct Cached {
    pipeline: wgpu::RenderPipeline,
    format: wgpu::TextureFormat,
    depth_format: Option<wgpu::TextureFormat>,
    samples: u32,
}

impl PipelineCache {
    pub fn new(layout: wgpu::PipelineLayout) -> Self {
        Self {
            layout,
            current: None,
        }
    }

    pub fn get<'a, F>(
        &'a mut self,
        format: wgpu::TextureFormat,
        depth_format: Option<wgpu::TextureFormat>,
        samples: u32,
        init: F,
    ) -> &'a wgpu::RenderPipeline
    where
        F: FnOnce(&wgpu::PipelineLayout) -> wgpu::RenderPipeline,
    {
        match &mut self.current {
            Some(existing)
                if existing.format == format
                    && existing.samples == samples
                    && existing.depth_format == depth_format =>
            {
                ()
            }
            _ => {
                let pipeline = init(&self.layout);

                self.current = Some(Cached {
                    pipeline,
                    format,
                    depth_format,
                    samples,
                });
            }
        }

        &self.current.as_ref().unwrap().pipeline
    }
}
