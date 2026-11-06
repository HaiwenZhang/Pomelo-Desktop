use super::*;

fn include(bounds: &mut Option<Bounds3>, other: Bounds3) {
    if let Some(bounds) = bounds {
        for i in 0..3 {
            bounds.min[i] = bounds.min[i].min(other.min[i]);
            bounds.max[i] = bounds.max[i].max(other.max[i]);
        }
    } else {
        *bounds = Some(other);
    }
}

impl Builder<'_> {
    fn tile(&self, point: [f64; 3]) -> [i64; 2] {
        [
            (point[0] / self.tile_mm).floor() as i64,
            (point[1] / self.tile_mm).floor() as i64,
        ]
    }

    fn weld(
        &self,
        solid: &PreparedSolid,
        owner: u32,
        absolute: bool,
    ) -> Result<Mesh3d, SolidPrepareError> {
        let scratch = solid
            .mesh
            .vertices
            .len()
            .saturating_mul(4 + 64 / 3)
            .saturating_add(solid.mesh.indices.len().saturating_mul(4));
        let limit = self.remaining()?.max_allocation_bytes;
        if scratch > limit {
            return Err(Self::error(SolidError::Limit {
                resource: "page remap bytes",
                actual: scratch,
                limit,
            }));
        }
        let mut vertices = Vec::new();
        vertices
            .try_reserve(solid.mesh.vertices.len() / 3)
            .map_err(|_| Self::error(SolidError::Allocation))?;
        let mut remap = Vec::new();
        remap
            .try_reserve_exact(solid.mesh.vertices.len())
            .map_err(|_| Self::error(SolidError::Allocation))?;
        let mut unique = HashMap::new();
        unique
            .try_reserve(solid.mesh.vertices.len() / 3)
            .map_err(|_| Self::error(SolidError::Allocation))?;
        for (i, v) in solid.mesh.vertices.iter().enumerate() {
            if i.is_multiple_of(1024) {
                self.cancel.check().map_err(|e| Self::error(e.into()))?;
            }
            let position = [
                (v.position[0] - if absolute { self.origin.x } else { 0.0 }) as f32,
                (v.position[1] - if absolute { self.origin.y } else { 0.0 }) as f32,
                v.position[2] as f32,
                1.0,
            ];
            if !position.into_iter().all(f32::is_finite) {
                return Err(Self::error(SolidError::Invalid("GPU_COORDINATES")));
            }
            let key = [
                position[0].to_bits(),
                position[1].to_bits(),
                position[2].to_bits(),
            ];
            let index = if let Some(&index) = unique.get(&key) {
                index
            } else {
                let index = u32::try_from(vertices.len())
                    .map_err(|_| Self::error(SolidError::Invalid("VERTEX_RANGE")))?;
                vertices.push(Vertex3d {
                    position,
                    normal: [v.normal[0], v.normal[1], v.normal[2], 0.0],
                    owner: [owner, 0, 0, 0],
                });
                unique.insert(key, index);
                index
            };
            remap.push(index);
        }
        let mut indices = Vec::new();
        indices
            .try_reserve_exact(solid.mesh.indices.len())
            .map_err(|_| Self::error(SolidError::Allocation))?;
        indices.extend(solid.mesh.indices.iter().map(|&i| remap[i as usize]));
        Ok(Mesh3d { vertices, indices })
    }

    fn triangle_bounds(&self, mesh: &Mesh3d, triangle: &[u32], absolute: bool) -> Bounds3 {
        let mut bounds = Bounds3 {
            min: [f64::INFINITY; 3],
            max: [f64::NEG_INFINITY; 3],
        };
        for &index in triangle {
            let p = mesh.vertices[index as usize].position;
            let p = [
                f64::from(p[0]) + if absolute { self.origin.x } else { 0.0 },
                f64::from(p[1]) + if absolute { self.origin.y } else { 0.0 },
                f64::from(p[2]),
            ];
            for (i, v) in p.into_iter().enumerate() {
                bounds.min[i] = bounds.min[i].min(v);
                bounds.max[i] = bounds.max[i].max(v);
            }
        }
        bounds
    }

    fn triangle_fits(&self, group: usize, triangle: &[u32], remap: &HashMap<u32, u32>) -> bool {
        let g = &self.groups[group];
        g.indices.len() + 3 <= PAGE_INDICES.min(self.limits.max_indices)
            && g.vertices.len() + triangle.iter().filter(|i| !remap.contains_key(i)).count()
                <= PAGE_VERTICES.min(self.limits.max_vertices)
    }

    fn triangle(
        &mut self,
        mesh: &Mesh3d,
        triangle: &[u32],
        group: usize,
        remap: &mut HashMap<u32, u32>,
        absolute: bool,
    ) -> Result<(), SolidPrepareError> {
        let bounds = self.triangle_bounds(mesh, triangle, absolute);
        let g = &mut self.groups[group];
        let before = g.allocation_bytes();
        g.vertices
            .try_reserve(3)
            .map_err(|_| Self::error(SolidError::Allocation))?;
        g.indices
            .try_reserve(3)
            .map_err(|_| Self::error(SolidError::Allocation))?;
        remap
            .try_reserve(3)
            .map_err(|_| Self::error(SolidError::Allocation))?;
        for &i in triangle {
            let index = if let Some(&index) = remap.get(&i) {
                index
            } else {
                let index = g.vertices.len() as u32;
                g.vertices.push(mesh.vertices[i as usize]);
                remap.insert(i, index);
                index
            };
            g.indices.push(index);
        }
        include(&mut g.bounds, bounds);
        self.group_bytes += g.allocation_bytes() - before;
        Ok(())
    }

    fn part(&mut self, source: u32, group: usize, start: u32) -> Result<(), SolidPrepareError> {
        let end = self.groups[group].indices.len() as u32;
        if start == end {
            return Ok(());
        }
        self.parts
            .try_reserve(1)
            .map_err(|_| Self::error(SolidError::Allocation))?;
        let before = self.groups[group].allocation_bytes();
        self.groups[group]
            .sources
            .try_reserve(1)
            .map_err(|_| Self::error(SolidError::Allocation))?;
        self.parts.push(SourcePart3d {
            mesh: group,
            indices: start..end,
        });
        self.groups[group].sources.push(source);
        self.group_bytes += self.groups[group].allocation_bytes() - before;
        Ok(())
    }

    pub(super) fn add_static(
        &mut self,
        result: Result<PreparedSolid, SolidPrepareError>,
        layer: LayerId,
    ) -> Result<(), SolidPrepareError> {
        let solid = match result {
            Ok(s) => s,
            Err(e) => return self.warning(e),
        };
        let Some(bounds) = solid.world_bounds() else {
            return Ok(());
        };
        let id = self.source(
            Identity {
                object: solid.object,
                layers: solid.layers,
                net: solid.net,
                placement: solid.placement,
                extension: 0.0,
            },
            bounds,
            [0.0, 0.0, 1.0, 0.0],
        )?;
        let mesh = self.weld(&solid, id, true)?;
        drop(solid);
        let mut tiles: BTreeMap<[i64; 2], Vec<u32>> = BTreeMap::new();
        for (i, triangle) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
            if i.is_multiple_of(1024) {
                self.cancel.check().map_err(|e| Self::error(e.into()))?;
            }
            let bounds = self.triangle_bounds(&mesh, triangle, true);
            let bin = tiles.entry(self.tile(bounds.center())).or_default();
            bin.try_reserve(3)
                .map_err(|_| Self::error(SolidError::Allocation))?;
            bin.extend(triangle);
        }
        for (tile, indices) in tiles {
            let mut remap = HashMap::new();
            let mut group = if let Some(&g) = self.static_groups.get(&(layer, tile)) {
                g
            } else {
                self.static_group(layer, tile)?
            };
            let mut start = self.groups[group].indices.len() as u32;
            for (i, triangle) in indices.as_chunks::<3>().0.iter().enumerate() {
                if i.is_multiple_of(1024) {
                    self.cancel.check().map_err(|e| Self::error(e.into()))?;
                }
                if !self.triangle_fits(group, triangle, &remap) {
                    self.part(id, group, start)?;
                    group = self.static_group(layer, tile)?;
                    start = 0;
                    remap.clear();
                    if !self.triangle_fits(group, triangle, &remap) {
                        return Err(Self::error(SolidError::Invalid("PAGE_TRIANGLE_LIMIT")));
                    }
                }
                self.triangle(&mesh, triangle, group, &mut remap, true)?;
            }
            self.part(id, group, start)?;
        }
        self.sources[id as usize - 1].parts.end = self.parts.len();
        self.remaining()?;
        Ok(())
    }

    fn static_group(&mut self, layer: LayerId, tile: [i64; 2]) -> Result<usize, SolidPrepareError> {
        let group = self.group()?;
        self.groups[group].instances.push(Instance3d {
            transform: [0.0, 0.0, 1.0, 0.0],
            owner: [0; 4],
        });
        self.group_bytes += self.groups[group].allocation_bytes();
        self.instance_bytes += self.groups[group].instances.capacity() * size_of::<Instance3d>();
        self.static_groups.insert((layer, tile), group);
        Ok(group)
    }

    pub(super) fn add_shared(
        &mut self,
        key: [u64; 16],
        meta: Identity,
        prepare: impl FnOnce(SolidLimits) -> Result<PreparedSolid, SolidPrepareError>,
    ) -> Result<(), SolidPrepareError> {
        let groups = if let Some(groups) = self.cache.get(&key) {
            groups.clone()
        } else {
            let solid = match prepare(self.remaining()?) {
                Ok(s) => s,
                Err(e) => return self.warning(e),
            };
            if solid.mesh.indices.is_empty() {
                return Ok(());
            }
            let bounds = solid
                .mesh
                .bounds
                .ok_or_else(|| Self::error(SolidError::Invalid("MESH_BOUNDS")))?;
            let mesh = self.weld(&solid, 0, false)?;
            drop(solid);
            let mut groups = Vec::new();
            if mesh.vertices.len() <= PAGE_VERTICES.min(self.limits.max_vertices)
                && mesh.indices.len() <= PAGE_INDICES.min(self.limits.max_indices)
            {
                let g = self.group()?;
                groups.push(g);
                self.groups[g].vertices = mesh.vertices;
                self.groups[g].indices = mesh.indices;
                self.groups[g].bounds = Some(bounds);
                self.groups[g].shared = true;
                self.group_bytes += self.groups[g].allocation_bytes();
            } else {
                let mut remap = HashMap::new();
                let mut group = self.group()?;
                groups.push(group);
                self.groups[group].shared = true;
                for (i, triangle) in mesh.indices.as_chunks::<3>().0.iter().enumerate() {
                    if i.is_multiple_of(1024) {
                        self.cancel.check().map_err(|e| Self::error(e.into()))?;
                    }
                    if !self.triangle_fits(group, triangle, &remap) {
                        group = self.group()?;
                        groups.push(group);
                        self.groups[group].shared = true;
                        remap.clear();
                        if !self.triangle_fits(group, triangle, &remap) {
                            return Err(Self::error(SolidError::Invalid("PAGE_TRIANGLE_LIMIT")));
                        }
                    }
                    self.triangle(&mesh, triangle, group, &mut remap, false)?;
                }
            }
            let groups: std::sync::Arc<[usize]> = groups.into();
            self.cache.insert(key, groups.clone());
            groups
        };
        let mut local = None;
        for &g in groups.iter() {
            if let Some(b) = self.groups[g].bounds {
                include(&mut local, b);
            }
        }
        let mut local = local.ok_or_else(|| Self::error(SolidError::Invalid("MESH_BOUNDS")))?;
        local.min[0] -= f64::from(meta.extension);
        local.max[0] += f64::from(meta.extension);
        let placement = meta.placement;
        let (s, c) = placement.angle.sin_cos();
        let transform = [
            (placement.at.x - self.origin.x) as f32,
            (placement.at.y - self.origin.y) as f32,
            c as f32,
            s as f32,
        ];
        let mut bounds = None;
        for p in local.corners() {
            let xy = Point::new(p[0], p[1]).rotate(placement.angle);
            let p = [xy.x + placement.at.x, xy.y + placement.at.y, p[2]];
            include(&mut bounds, Bounds3 { min: p, max: p });
        }
        let extension = meta.extension;
        let id = self.source(
            meta,
            bounds.ok_or_else(|| Self::error(SolidError::Invalid("SOURCE_BOUNDS")))?,
            transform,
        )?;
        for &g in groups.iter() {
            let before = self.groups[g].allocation_bytes();
            let instances_before = self.groups[g].instances.capacity() * size_of::<Instance3d>();
            self.parts
                .try_reserve(1)
                .map_err(|_| Self::error(SolidError::Allocation))?;
            self.groups[g]
                .sources
                .try_reserve(1)
                .map_err(|_| Self::error(SolidError::Allocation))?;
            self.groups[g]
                .instances
                .try_reserve(1)
                .map_err(|_| Self::error(SolidError::Allocation))?;
            self.parts.push(SourcePart3d {
                mesh: g,
                indices: 0..self.groups[g].indices.len() as u32,
            });
            self.groups[g].sources.push(id);
            self.groups[g].instances.push(Instance3d {
                transform,
                owner: [id, u32::from(extension != 0.0), extension.to_bits(), 0],
            });
            self.group_bytes += self.groups[g].allocation_bytes() - before;
            self.instance_bytes +=
                self.groups[g].instances.capacity() * size_of::<Instance3d>() - instances_before;
        }
        self.sources[id as usize - 1].parts.end = self.parts.len();
        self.remaining()?;
        Ok(())
    }

    pub(super) fn finish(
        self,
        layers: Vec<LayerId>,
        preview: PreviewStackup,
    ) -> Result<PreparedBoard3d, SolidPrepareError> {
        self.remaining()?;
        let mut meshes = Vec::new();
        let mut pages = Vec::new();
        let mut scene_bounds = None;
        meshes
            .try_reserve_exact(self.groups.len())
            .map_err(|_| Self::error(SolidError::Allocation))?;
        for source in &self.sources {
            include(&mut scene_bounds, source.bounds);
        }
        for (mesh, mut group) in self.groups.into_iter().enumerate() {
            group.vertices.shrink_to_fit();
            group.indices.shrink_to_fit();
            self.cancel.check().map_err(|e| Self::error(e.into()))?;
            meshes.push(Mesh3d {
                vertices: group.vertices,
                indices: group.indices,
            });
            if !group.shared {
                pages
                    .try_reserve(1)
                    .map_err(|_| Self::error(SolidError::Allocation))?;
                pages.push(Page3d {
                    mesh,
                    instances: group.instances,
                    sources: group.sources,
                    bounds: group
                        .bounds
                        .ok_or_else(|| Self::error(SolidError::Invalid("PAGE_BOUNDS")))?,
                });
                continue;
            }
            let mut tiles: BTreeMap<[i64; 2], Vec<(u32, Instance3d)>> = BTreeMap::new();
            for (id, instance) in group.sources.into_iter().zip(group.instances) {
                let center = self.sources[id as usize - 1].bounds.center();
                let tile = [
                    (center[0] / self.tile_mm).floor() as i64,
                    (center[1] / self.tile_mm).floor() as i64,
                ];
                let bin = tiles.entry(tile).or_default();
                bin.try_reserve(1)
                    .map_err(|_| Self::error(SolidError::Allocation))?;
                bin.push((id, instance));
            }
            for bin in tiles.into_values() {
                for chunk in bin.chunks(PAGE_INSTANCES) {
                    let mut bounds = None;
                    let mut sources = Vec::new();
                    let mut instances = Vec::new();
                    sources
                        .try_reserve_exact(chunk.len())
                        .map_err(|_| Self::error(SolidError::Allocation))?;
                    instances
                        .try_reserve_exact(chunk.len())
                        .map_err(|_| Self::error(SolidError::Allocation))?;
                    for &(id, instance) in chunk {
                        include(&mut bounds, self.sources[id as usize - 1].bounds);
                        sources.push(id);
                        instances.push(instance);
                    }
                    pages
                        .try_reserve(1)
                        .map_err(|_| Self::error(SolidError::Allocation))?;
                    pages.push(Page3d {
                        mesh,
                        instances,
                        sources,
                        bounds: bounds
                            .ok_or_else(|| Self::error(SolidError::Invalid("PAGE_BOUNDS")))?,
                    });
                }
            }
        }
        let base_bytes = meshes
            .iter()
            .map(|m| m.vertices.capacity() * size_of::<Vertex3d>() + m.indices.capacity() * 4)
            .sum::<usize>()
            + pages
                .iter()
                .map(|p: &Page3d| {
                    p.instances.capacity() * size_of::<Instance3d>() + p.sources.capacity() * 4
                })
                .sum::<usize>()
            + self.sources.capacity() * size_of::<Source3d>()
            + self.parts.capacity() * size_of::<SourcePart3d>();
        let index_bytes = spatial::SpatialIndex3d::estimate_bytes(self.sources.len())
            .saturating_add(spatial::SpatialIndex3d::estimate_bytes(pages.len()));
        if base_bytes.saturating_add(index_bytes) > self.limits.max_allocation_bytes {
            return Err(Self::error(SolidError::Limit {
                resource: "spatial index bytes",
                actual: base_bytes.saturating_add(index_bytes),
                limit: self.limits.max_allocation_bytes,
            }));
        }
        let source_index = spatial::SpatialIndex3d::build(
            self.sources.len(),
            |i| self.sources[i].bounds,
            self.cancel,
        )?;
        let page_index =
            spatial::SpatialIndex3d::build(pages.len(), |i| pages[i].bounds, self.cancel)?;
        let scene = PreparedBoard3d {
            source_index,
            page_index,
            memory_reservation: None,
            meshes,
            pages,
            parts: self.parts,
            sources: self.sources,
            origin: self.origin,
            bounds: scene_bounds
                .ok_or_else(|| Self::error(SolidError::Invalid("EMPTY_3D_BOARD")))?,
            layers,
            warnings: self.warnings,
            warning_count: self.warning_count,
            preview,
        };
        if scene.allocation_bytes() > self.limits.max_allocation_bytes {
            return Err(Self::error(SolidError::Limit {
                resource: "bytes",
                actual: scene.allocation_bytes(),
                limit: self.limits.max_allocation_bytes,
            }));
        }
        Ok(scene)
    }
}
