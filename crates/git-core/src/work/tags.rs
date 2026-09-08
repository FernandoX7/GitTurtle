//! Bounded tag discovery and captured, explicit local or named-remote actions.
use super::*;

const TAG_LIMIT: usize = 10_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Tag {
    pub name: String,
    /// The tag object for an annotated tag, otherwise the directly named object.
    pub oid: String,
    pub target_oid: String,
    pub target_kind: String,
    pub annotated: bool,
    pub tagger: String,
    root: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagList {
    pub tags: Vec<Tag>,
    pub truncated: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TagDetails {
    pub tag: Tag,
    pub message: String,
    pub annotation_unavailable: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CreateTagPlan {
    pub name: String,
    pub target_oid: String,
    pub annotation: Option<String>,
    pub signing: bool,
    root: PathBuf,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TagCommand {
    Create(CreateTagPlan),
    Delete(Tag),
    /// Only the named captured tag is pushed, with no force and no follow-tags.
    Push {
        tag: Tag,
        remote: Arc<RemoteConfig>,
    },
}

impl GitRepository {
    pub fn tags(&self) -> Result<TagList> {
        let bytes = run_git(
            &self.path,
            &[
                "for-each-ref",
                "--sort=refname",
                "--count=10001",
                "--format=%(refname)%00%(objectname)%00%(objecttype)%00%(*objectname)%00%(*objecttype)%00%(taggername)%00%(symref)",
                "refs/tags/",
            ],
        )?;
        let mut tags = bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
            .map(|line| self.parse_tag(line))
            .collect::<Result<Vec<_>>>()?;
        let truncated = tags.len() > TAG_LIMIT;
        tags.truncate(TAG_LIMIT);
        Ok(TagList { tags, truncated })
    }

    pub fn tag_details(&self, expected: &Tag) -> Result<TagDetails> {
        self.check_tag(expected)?;
        if expected.annotated {
            let size = run_git(&self.path, &["cat-file", "-s", &expected.oid])?;
            let size = text(trim_line(&size))
                .parse::<u64>()
                .context("Invalid tag object size")?;
            if size > 256 * 1024 {
                return Ok(TagDetails { tag: expected.clone(), message: String::new(), annotation_unavailable: Some("This tag annotation exceeds the 256 KiB inspector limit. Its identity and tag actions remain available.".into()) });
            }
        }
        let message = if expected.annotated {
            let bytes = run_git(&self.path, &["cat-file", "tag", &expected.oid])?;
            // An annotation is repository data; bound its inspector independently.
            ensure!(
                bytes.len() <= 256 * 1024,
                "This tag annotation exceeds the 256 KiB inspector limit."
            );
            bytes
                .windows(2)
                .position(|window| window == b"\n\n")
                .map(|index| text(&bytes[index + 2..]))
                .unwrap_or_default()
        } else {
            String::new()
        };
        Ok(TagDetails {
            tag: expected.clone(),
            message,
            annotation_unavailable: None,
        })
    }

    pub fn create_tag_plan(
        &self,
        name: &str,
        target: &str,
        annotation: Option<String>,
    ) -> Result<CreateTagPlan> {
        self.validate_tag_name(name)?;
        ensure!(
            self.tag_named(name)?.is_none(),
            "A local tag named '{name}' already exists. Choose another name."
        );
        ensure!(
            !target.is_empty() && target.len() <= 4096 && !target.starts_with('-'),
            "Choose a commit for this tag."
        );
        let target_oid = text(trim_line(&run_git(
            &self.path,
            &[
                "rev-parse",
                "--verify",
                "--end-of-options",
                &format!("{target}^{{commit}}"),
            ],
        )?));
        validate_oid(&target_oid)?;
        if let Some(message) = &annotation {
            ensure!(
                !message.trim().is_empty()
                    && message.len() <= 256 * 1024
                    && !message.contains('\0'),
                "An annotated tag needs a message up to 256 KiB without NUL bytes."
            );
        }
        let signing = self.tag_signing()?;
        ensure!(
            annotation.is_some() || !signing,
            "Git is configured to sign tags. Choose Annotated and enter a message so that signing remains enabled."
        );
        Ok(CreateTagPlan {
            name: name.into(),
            target_oid,
            annotation,
            signing,
            root: self.path.clone(),
        })
    }

    pub(super) fn execute_tag(&self, operation: &TagCommand) -> Result<WriteOutcome> {
        let mut command = normal_command(&self.path);
        let mut input = None;
        let mut timeout = WRITE_TIMEOUT;
        let message = match operation {
            TagCommand::Create(plan) => {
                ensure!(
                    plan.root == self.path
                        && self.create_tag_plan(
                            &plan.name,
                            &plan.target_oid,
                            plan.annotation.clone()
                        )? == *plan,
                    "The tag name or signing settings changed; review creation again."
                );
                command.arg("tag");
                if let Some(annotation) = &plan.annotation {
                    command.args(["--annotate", "--cleanup=verbatim", "--file=-"]);
                    input = Some(annotation.as_bytes().to_vec());
                }
                command.args(["--", &plan.name, &plan.target_oid]);
                format!(
                    "Created local tag '{}' at {}",
                    plan.name,
                    &plan.target_oid[..8]
                )
            }
            TagCommand::Delete(tag) => {
                self.check_tag(tag)?;
                // update-ref compares and deletes under Git's reference lock, so
                // a tag moved after the review is never silently removed.
                command.args([
                    "update-ref",
                    "--no-deref",
                    "-d",
                    &format!("refs/tags/{}", tag.name),
                    &tag.oid,
                ]);
                format!(
                    "Deleted local tag '{}'. Remote tags are unchanged.",
                    tag.name
                )
            }
            TagCommand::Push { tag, remote } => {
                self.check_tag(tag)?;
                let actual = self
                    .remote_configs()?
                    .into_iter()
                    .find(|candidate| candidate.name == remote.name);
                ensure!(
                    actual.as_ref() == Some(remote.as_ref()),
                    "The remote configuration changed; review this tag's destination again."
                );
                self.validate_remote(&remote.name)?;
                let mut urls = normal_command(&self.path);
                urls.args(["remote", "get-url", "--push", "--all", "--", &remote.name]);
                ensure!(
                    text(&checked_write_output(urls, None, GIT_TIMEOUT)?.stdout)
                        .lines()
                        .count()
                        == 1,
                    "Choose a remote with exactly one push destination."
                );
                configure_network(&mut command, self)?;
                command.args([
                    "-c",
                    &format!("remote.{}.mirror=false", remote.name),
                    "-c",
                    "push.followTags=false",
                    "push",
                    "--porcelain",
                    "--progress",
                    "--no-force",
                    "--no-force-with-lease",
                    "--no-follow-tags",
                    "--recurse-submodules=no",
                    "--",
                    &remote.name,
                    &format!("{}:refs/tags/{}", tag.oid, tag.name),
                ]);
                timeout = NETWORK_TIMEOUT;
                format!("Pushed only tag '{}' to '{}'", tag.name, remote.name)
            }
        };
        checked_write_output(command, input, timeout)?;
        Ok(WriteOutcome {
            message,
            commit_oid: None,
        })
    }

    fn tag_signing(&self) -> Result<bool> {
        let mut command = normal_command(&self.path);
        command.args(["config", "--bool", "--get", "tag.gpgSign"]);
        let output = bounded_write_output(command, None, GIT_TIMEOUT)?;
        ensure!(
            output.status.success() || output.status.code() == Some(1),
            "Unable to read tag signing configuration: {}",
            text(&output.stderr)
        );
        Ok(trim_line(&output.stdout) == b"true")
    }

    fn validate_tag_name(&self, name: &str) -> Result<()> {
        ensure!(
            !name.is_empty() && name.len() <= 4096 && !name.starts_with('-'),
            "Enter a tag name up to 4096 bytes that does not begin with '-'."
        );
        run_git(
            &self.path,
            &["check-ref-format", &format!("refs/tags/{name}")],
        )?;
        Ok(())
    }

    fn tag_named(&self, name: &str) -> Result<Option<Tag>> {
        self.validate_tag_name(name)?;
        let reference = format!("refs/tags/{name}");
        let bytes = run_git(
            &self.path,
            &[
                "for-each-ref",
                "--count=2",
                "--format=%(refname)%00%(objectname)%00%(objecttype)%00%(*objectname)%00%(*objecttype)%00%(taggername)%00%(symref)",
                &reference,
            ],
        )?;
        for line in bytes
            .split(|byte| *byte == b'\n')
            .filter(|line| !line.is_empty())
        {
            let tag = self.parse_tag(line)?;
            if tag.name == name {
                return Ok(Some(tag));
            }
        }
        Ok(None)
    }

    fn check_tag(&self, expected: &Tag) -> Result<()> {
        ensure!(
            expected.root == self.path
                && self.tag_named(&expected.name)?.as_ref() == Some(expected),
            "This tag moved or disappeared; refresh the tag list and review it again."
        );
        Ok(())
    }

    fn parse_tag(&self, line: &[u8]) -> Result<Tag> {
        let fields = line
            .split(|byte| *byte == 0)
            .map(std::str::from_utf8)
            .collect::<std::result::Result<Vec<_>, _>>()?;
        ensure!(
            fields.len() == 7 && fields[6].is_empty(),
            "Unsupported or symbolic tag reference."
        );
        validate_oid(fields[1])?;
        let annotated = fields[2] == "tag";
        let target_oid = if annotated { fields[3] } else { fields[1] };
        validate_oid(target_oid)?;
        Ok(Tag {
            name: fields[0]
                .strip_prefix("refs/tags/")
                .context("Invalid tag reference")?
                .into(),
            oid: fields[1].into(),
            target_oid: target_oid.into(),
            target_kind: (if annotated { fields[4] } else { fields[2] }).into(),
            annotated,
            tagger: fields[5].into(),
            root: self.path.clone(),
        })
    }
}
