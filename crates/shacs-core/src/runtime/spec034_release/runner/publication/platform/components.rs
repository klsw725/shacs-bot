use super::*;

impl EvidenceDestination {
    pub(super) fn open_components(
        &mut self,
        after_mkdir: &mut impl FnMut(&OsStr) -> Result<(), Spec034ReleaseArtifactError>,
    ) -> Result<(), Spec034ReleaseArtifactError> {
        for name in &self.components {
            let parent_index = self.handles.len() - 1;
            let (created, child) = match open_child(&self.handles[parent_index], name) {
                Ok(child) => (false, child),
                Err(error) if error == rustix::io::Errno::NOENT => {
                    mkdirat(
                        &self.handles[parent_index],
                        name,
                        Mode::from_raw_mode(0o700),
                    )
                    .map_err(|_| Spec034ReleaseArtifactError::InvalidConfig)?;
                    if let Err(error) = after_mkdir(name) {
                        let _ = unlinkat(&self.handles[parent_index], name, AtFlags::REMOVEDIR);
                        return Err(error);
                    }
                    if fsync(&self.handles[parent_index]).is_err() {
                        let _ = unlinkat(&self.handles[parent_index], name, AtFlags::REMOVEDIR);
                        return Err(Spec034ReleaseArtifactError::CommitStatusUnknown(
                            PublicationStage::DirectorySync,
                        ));
                    }
                    let child = match open_child(&self.handles[parent_index], name) {
                        Ok(child) => child,
                        Err(_) => {
                            let _ = unlinkat(
                                &self.handles[parent_index],
                                name,
                                AtFlags::REMOVEDIR,
                            );
                            return Err(Spec034ReleaseArtifactError::InvalidConfig);
                        }
                    };
                    (true, child)
                }
                Err(_) => return Err(Spec034ReleaseArtifactError::InvalidConfig),
            };
            self.handles.push(child.into());
            if created {
                self.created.push(CreatedComponent {
                    parent_index,
                    name: name.clone(),
                });
            }
        }
        Ok(())
    }
}

impl Drop for EvidenceDestination {
    fn drop(&mut self) {
        if self.published {
            return;
        }
        for created in self.created.iter().rev() {
            let parent = &self.handles[created.parent_index];
            if same_handle_path(
                parent,
                &created.name,
                &self.handles[created.parent_index + 1],
            ) {
                let _ = unlinkat(parent, &created.name, AtFlags::REMOVEDIR);
            }
        }
    }
}
