use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};
use tree_sitter::{Parser, Query, QueryCursor, StreamingIterator};

const FILE_COUNT_MAX: usize = 64;
const SOURCE_BYTES_MAX: usize = 4 * 1024 * 1024;

pub fn source_line(lua: &mlua::Lua) -> usize {
    for level in 1..256 {
        let Some(line) = lua.inspect_stack(level, |debug| {
            if debug.source().source.as_deref() == Some("@model.luau") {
                debug.current_line()
            } else {
                None
            }
        }) else {
            break;
        };
        if let Some(line) = line {
            return line.saturating_sub(1);
        }
    }
    0
}

#[derive(Clone)]
struct File {
    path: PathBuf,
    source: std::sync::Arc<str>,
    imports: Vec<(std::ops::Range<usize>, usize)>,
}

#[derive(Clone)]
pub struct Project {
    files: Vec<File>,
    order: Vec<usize>,
    paths: Vec<PathBuf>,
}

impl Project {
    pub fn load_if_imported(path: &Path, source: &str) -> Result<Option<Self>, String> {
        if !source.contains("require") {
            return Ok(None);
        }
        if source.len() > SOURCE_BYTES_MAX {
            let mut parser = Parser::new();
            parser
                .set_language(&tree_sitter_luau::LANGUAGE.into())
                .map_err(|error| error.to_string())?;
            let tree = parser
                .parse(source, None)
                .ok_or("Cannot parse model imports")?;
            let query = Query::new(
                &tree_sitter_luau::LANGUAGE.into(),
                "((function_call name: (identifier) @name) (#eq? @name \"require\"))",
            )
            .map_err(|error| error.to_string())?;
            let mut cursor = QueryCursor::new();
            cursor.set_match_limit(4096);
            let imported = cursor
                .matches(&query, tree.root_node(), source.as_bytes())
                .next()
                .is_some();
            if cursor.did_exceed_match_limit() {
                return Err("Model import query exceeded its limit".into());
            }
            if !imported {
                return Ok(None);
            }
        }
        Self::load(path, source).map(Some)
    }
    pub fn load(path: &Path, source: &str) -> Result<Self, String> {
        if source.len() > SOURCE_BYTES_MAX {
            return Err("Project source exceeds 4 MiB".into());
        }
        let absolute = if path.is_absolute() {
            path.to_owned()
        } else {
            std::env::current_dir()
                .map_err(|error| error.to_string())?
                .join(path)
        };
        let root = absolute
            .parent()
            .ok_or("Model needs a project directory")?
            .canonicalize()
            .map_err(|error| error.to_string())?;
        let entry = root.join(absolute.file_name().ok_or("Model needs a file name")?);
        let mut files = vec![File {
            path: entry.clone(),
            source: source.into(),
            imports: Vec::new(),
        }];
        let mut indices = HashMap::from([(entry, 0)]);
        let mut bytes = source.len();
        let mut paths = Vec::new();
        let mut parser = Parser::new();
        parser
            .set_language(&tree_sitter_luau::LANGUAGE.into())
            .map_err(|error| error.to_string())?;
        let query = Query::new(
            &tree_sitter_luau::LANGUAGE.into(),
            "(function_call name: (identifier) @name arguments: (arguments) @arguments) @call",
        )
        .map_err(|error| error.to_string())?;
        let mut index = 0;
        while index < files.len() {
            if bytes > SOURCE_BYTES_MAX {
                return Err("Project source exceeds 4 MiB".into());
            }
            let file = &files[index];
            let tree = parser
                .parse(file.source.as_bytes(), None)
                .ok_or("Cannot parse project module")?;
            let mut cursor = QueryCursor::new();
            cursor.set_match_limit(4096);
            let mut matches = cursor.matches(&query, tree.root_node(), file.source.as_bytes());
            let mut imports = Vec::new();
            for _ in 0..4096 {
                let Some(found) = matches.next() else {
                    break;
                };
                let name = found
                    .captures
                    .iter()
                    .find(|capture| query.capture_names()[capture.index as usize] == "name")
                    .expect("Name capture")
                    .node;
                if &file.source[name.byte_range()] != "require" {
                    continue;
                }
                let arguments = found
                    .captures
                    .iter()
                    .find(|capture| query.capture_names()[capture.index as usize] == "arguments")
                    .expect("Arguments capture")
                    .node;
                let call = found
                    .captures
                    .iter()
                    .find(|capture| query.capture_names()[capture.index as usize] == "call")
                    .expect("Call capture")
                    .node;
                let argument = file.source[arguments.byte_range()]
                    .trim()
                    .trim_start_matches('(')
                    .trim_end_matches(')')
                    .trim();
                let Some(quote) = argument
                    .chars()
                    .next()
                    .filter(|quote| ['\'', '"'].contains(quote))
                else {
                    return Err("require needs a literal relative module path".into());
                };
                let relative = argument
                    .strip_prefix(quote)
                    .and_then(|value| value.strip_suffix(quote))
                    .ok_or("Invalid require path")?;
                if !relative.starts_with("./") || relative.contains(['\\', '\'', '"', '\0']) {
                    return Err("Module paths must start with ./ and contain no escapes".into());
                }
                let mut target = file.path.parent().expect("Module directory").join(relative);
                if target.extension().is_none() {
                    target.set_extension("luau");
                }
                if target
                    .extension()
                    .is_none_or(|extension| extension != "luau")
                {
                    return Err("Modules must use .luau files".into());
                }
                if paths.len() >= 4096 {
                    return Err("Project exceeds 4096 imports".into());
                }
                paths.push(target.clone());
                let target = target
                    .canonicalize()
                    .map_err(|error| format!("Module {relative}: {error}"))?;
                if !target.starts_with(&root) {
                    return Err(format!("Module {relative} escapes the project directory"));
                }
                imports.push((call.byte_range(), target));
            }
            if matches.next().is_some() {
                return Err("Project source exceeds 4096 function calls".into());
            }
            if cursor.did_exceed_match_limit() {
                return Err("Project import query exceeded its limit".into());
            }
            let mut resolved = Vec::new();
            for (range, target) in imports {
                let dependency = if let Some(index) = indices.get(&target) {
                    *index
                } else {
                    if files.len() >= FILE_COUNT_MAX {
                        return Err("Project exceeds 64 files".into());
                    }
                    let source = crate::model::read_source(&target)?;
                    bytes = bytes
                        .checked_add(source.len())
                        .ok_or("Project size overflow")?;
                    if bytes > SOURCE_BYTES_MAX {
                        return Err("Project source exceeds 4 MiB".into());
                    }
                    let next = files.len();
                    indices.insert(target.clone(), next);
                    files.push(File {
                        path: target,
                        source: source.into(),
                        imports: Vec::new(),
                    });
                    next
                };
                resolved.push((range, dependency));
            }
            files[index].imports = resolved;
            index += 1;
        }
        let mut order = Vec::new();
        let mut complete = vec![false; files.len()];
        for _ in 0..files.len() {
            let before = order.len();
            for index in 0..files.len() {
                if !complete[index]
                    && files[index]
                        .imports
                        .iter()
                        .all(|(_, dependency)| complete[*dependency])
                {
                    complete[index] = true;
                    order.push(index);
                }
            }
            if order.len() == files.len() {
                break;
            }
            if order.len() == before {
                return Err("Cyclic module dependency".into());
            }
        }
        Ok(Self {
            files,
            order,
            paths,
        })
    }

    pub fn dependencies(&self) -> impl Iterator<Item = &Path> {
        self.paths.iter().map(PathBuf::as_path)
    }

    pub fn dependency_fingerprint(&self) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hash = std::collections::hash_map::DefaultHasher::new();
        for file in self.files.iter().skip(1) {
            file.path.hash(&mut hash);
            file.source.hash(&mut hash);
        }
        hash.finish()
    }

    pub fn sources(&self, entry: &str) -> Vec<(String, String)> {
        self.files
            .iter()
            .enumerate()
            .map(|(index, file)| {
                let name = if index == 0 {
                    entry.to_owned()
                } else {
                    format!("module-{index}.luau")
                };
                let mut source = String::new();
                let mut previous = 0;
                for (range, dependency) in &file.imports {
                    source.push_str(&file.source[previous..range.start]);
                    source.push_str(&format!("require('./module-{dependency}')"));
                    source.extend(std::iter::repeat_n(
                        '\n',
                        file.source[range.clone()]
                            .bytes()
                            .filter(|byte| *byte == b'\n')
                            .count(),
                    ));
                    previous = range.end;
                }
                source.push_str(&file.source[previous..]);
                (name, source)
            })
            .collect()
    }

    pub fn original_sources(&self) -> impl Iterator<Item = &str> {
        self.files.iter().map(|file| file.source.as_ref())
    }

    pub fn analysis_key(&self, root: &str) -> String {
        let mut key = String::new();
        for (index, file) in self.files.iter().enumerate() {
            key.push_str(&format!("\0{}\0", file.path.display()));
            if index == 0 {
                key.push_str(root);
            } else {
                key.push_str(&file.source);
            }
        }
        key
    }

    pub fn install(&self, lua: &mlua::Lua) -> mlua::Result<()> {
        let require_factory: mlua::Function = lua
            .load(
                r#"
return function(imports)
    return function(path)
        local module = imports[path]
        if module == nil then error("require needs a declared literal relative module path") end
        return module()
    end
end
"#,
            )
            .set_name("@spice-module-imports")
            .eval()?;
        let module_factory: mlua::Function = lua
            .load(
                r#"
return function(loader, name)
    local loaded = false
    local value = nil
    return function()
        if not loaded then
            local result = loader()
            if result == nil then error("Module " .. name .. " must return a value") end
            value = result
            loaded = true
        end
        return value
    end
end
"#,
            )
            .set_name("@spice-module-cache")
            .eval()?;
        let mut values = HashMap::<usize, mlua::Function>::new();
        for index in &self.order {
            let file = &self.files[*index];
            let imports = lua.create_table()?;
            for (range, dependency) in &file.imports {
                let call = &file.source[range.clone()];
                let start = call
                    .find(['\'', '"'])
                    .ok_or_else(|| mlua::Error::runtime("Invalid module literal"))?;
                let quote = call.as_bytes()[start] as char;
                let relative = call[start + 1..].split(quote).next().expect("Literal path");
                imports.set(
                    relative,
                    values
                        .get(dependency)
                        .expect("Topological module order")
                        .clone(),
                )?;
            }
            let require: mlua::Function = require_factory.call(imports)?;
            if *index == 0 {
                lua.globals().set("require", require)?;
            } else {
                let environment = lua.create_table()?;
                let metatable = lua.create_table()?;
                metatable.set("__index", lua.globals())?;
                environment.set_metatable(Some(metatable))?;
                environment.set("require", require)?;
                let loader = lua
                    .load(file.source.as_ref())
                    .set_name(format!("@{}", file.path.display()))
                    .set_environment(environment)
                    .into_function()?;
                let value: mlua::Function =
                    module_factory.call((loader, file.path.display().to_string()))?;
                values.insert(*index, value);
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let path = std::env::temp_dir().join(format!(
                "spice-modules-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir(&path).expect("Fixture directory");
            Self(path)
        }
        fn write(&self, name: &str, source: &str) {
            std::fs::write(self.0.join(name), source).expect("Fixture source");
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).expect("Fixture cleanup");
        }
    }

    #[test]
    fn modules_are_typed_cached_per_run_and_import_edits_invalidate_analysis() -> Result<(), String>
    {
        let fixture = Fixture::new();
        fixture.write("part.luau", "local count = 0\nreturn function(width: number)\n count += 1\n return cad.box({width=width,depth=count,height=2})\nend");
        let source = "local first = require('./part')\nlocal second = require('./part')\ncad.show('a', first(10))\ncad.show('b', second(10))";
        let path = fixture.0.join("main.luau");
        let project = Project::load(&path, source)?;
        let mut analysis = crate::analysis::Cache::default();
        let model = crate::model::compile_project_cached(source, Some(&project), &mut analysis)?;
        assert_eq!(model.parts.len(), 2);
        assert_eq!(model.parts[0].line, 2);
        let kernel = crate::kernel::Kernel::new()?;
        let bodies = model.build(&kernel)?;
        assert!((bodies[0].volume()? - 20.0).abs() < 0.001);
        assert!((bodies[1].volume()? - 40.0).abs() < 0.001);

        fixture.write(
            "part.luau",
            "return function(width: string) return cad.box({width=width,depth=1,height=2}) end",
        );
        let changed = Project::load(&path, source)?;
        assert!(
            crate::model::compile_project_cached(source, Some(&changed), &mut analysis).is_err()
        );
        Ok(())
    }

    #[test]
    fn rejects_cycles_missing_dynamic_and_escaping_modules() -> Result<(), String> {
        let fixture = Fixture::new();
        let path = fixture.0.join("main.luau");
        fixture.write("a.luau", "return require('./b')");
        fixture.write("b.luau", "return require('./a')");
        assert!(
            Project::load(&path, "require('./a')")
                .err()
                .expect("Cycle")
                .contains("Cyclic")
        );
        for source in [
            "require('./missing')",
            "require('os')",
            "local name='./a'; require(name)",
            "require('./../outside.luau')",
        ] {
            assert!(Project::load(&path, source).is_err(), "{source}");
        }
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink("/etc/passwd", fixture.0.join("escape.luau"))
                .map_err(|error| error.to_string())?;
            assert!(
                Project::load(&path, "require('./escape')")
                    .err()
                    .expect("Escape")
                    .contains("escapes")
            );
        }
        Ok(())
    }

    #[test]
    fn imported_files_are_watched_including_unsaved_imports_and_missing_file_recovery()
    -> Result<(), String> {
        let fixture = Fixture::new();
        let path = fixture.0.join("main.luau");
        fixture.write("main.luau", "return 1");
        fixture.write("part.luau", "return 1");
        let watcher =
            crate::document::Watcher::new(path.clone(), eframe::egui::Context::default())?;
        watcher
            .events
            .recv_timeout(std::time::Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        watcher.source("local part=require('./part'); return part".into());
        std::thread::sleep(std::time::Duration::from_millis(350));
        fixture.write("part.luau", "return 200");
        watcher
            .events
            .recv_timeout(std::time::Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        std::fs::remove_file(fixture.0.join("part.luau")).map_err(|error| error.to_string())?;
        watcher
            .events
            .recv_timeout(std::time::Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        fixture.write("part.luau", "return 300");
        watcher
            .events
            .recv_timeout(std::time::Duration::from_secs(2))
            .map_err(|error| error.to_string())?;
        drop(watcher);
        Ok(())
    }

    #[test]
    fn project_limits_bound_files_and_source() {
        let fixture = Fixture::new();
        for index in 0..64 {
            let source = if index == 63 {
                "return 1".to_owned()
            } else {
                format!("return require('./part{}')", index + 1)
            };
            fixture.write(&format!("part{index}.luau"), &source);
        }
        assert!(
            Project::load(&fixture.0.join("main.luau"), "return require('./part0')")
                .err()
                .expect("File limit")
                .contains("64 files")
        );
        let calls = format!("{}require('./part63')", "print('bounded')\n".repeat(4096));
        assert!(
            Project::load(&fixture.0.join("main.luau"), &calls)
                .err()
                .expect("Call limit")
                .contains("4096 function calls")
        );
        assert!(
            Project::load_if_imported(
                Path::new("/missing/project/main.luau"),
                "cad.show('box',cad.box({width=1,depth=1,height=1}))"
            )
            .expect("No filesystem work without imports")
            .is_none()
        );
        assert!(
            Project::load(
                &fixture.0.join("main.luau"),
                &" ".repeat(SOURCE_BYTES_MAX + 1)
            )
            .err()
            .expect("Source limit")
            .contains("4 MiB")
        );
    }

    #[test]
    fn exported_types_and_captured_modules_survive_print_and_game_export() -> Result<(), String> {
        let fixture = Fixture::new();
        let source = "local component=require('./component')\nlocal options: component.Options = {width=12}\ncad.show('typed', component.build(options))";
        fixture.write("main.luau", source);
        fixture.write("component.luau", "export type Options = {width: number}\nreturn {build=function(options: Options) return cad.box({width=options.width,depth=3,height=2}) end}");
        let path = fixture.0.join("main.luau");
        let project = Project::load(&path, source)?;
        let mut analysis = crate::analysis::Cache::default();
        let model = crate::model::compile_project_cached(source, Some(&project), &mut analysis)?;
        assert_eq!(model.name, "typed");
        fixture.write("component.luau", "error('Changed after snapshot')");
        for (name, target) in [
            ("print", crate::export::Target::Print),
            ("game", crate::export::Target::Game),
        ] {
            let folder = fixture.0.join(name);
            crate::export::model_files_project(source, Some(&project), &folder, target)?;
            if target == crate::export::Target::Game {
                let metadata: serde_json::Value = serde_json::from_slice(
                    &std::fs::read(folder.join("asset.json")).map_err(|error| error.to_string())?,
                )
                .map_err(|error| error.to_string())?;
                let sources = metadata["files"]["sources"]
                    .as_array()
                    .expect("Source inventory");
                assert_eq!(sources.len(), 2);
                for file in sources {
                    let path = file["path"].as_str().expect("Bundled source path");
                    let bytes =
                        std::fs::read(folder.join(path)).map_err(|error| error.to_string())?;
                    assert_eq!(file["sha256"], crate::game_contract::hash(&bytes));
                }
                assert_eq!(
                    sources[0]["input_sha256"],
                    crate::game_contract::hash(source.as_bytes())
                );
                assert!(
                    !std::fs::read_to_string(folder.join("module-1.luau"))
                        .map_err(|error| error.to_string())?
                        .contains("Changed after snapshot")
                );
            }
            let exported = crate::model::read_source(&folder.join("model.luau"))?;
            let captured = Project::load(&folder.join("model.luau"), &exported)?;
            let model =
                crate::model::compile_project_cached(&exported, Some(&captured), &mut analysis)?;
            let kernel = crate::kernel::Kernel::new()?;
            assert!((model.build(&kernel)?[0].volume()? - 72.0).abs() < 0.001);
        }
        Ok(())
    }

    #[test]
    fn module_outputs_navigate_to_root_imports_and_factory_calls() -> Result<(), String> {
        let fixture = Fixture::new();
        fixture.write("part.luau", &format!("{}cad.show('eager',cad.box({{width=1,depth=1,height=1}}))\nreturn function() cad.show('factory',cad.box({{width=2,depth=1,height=1}})) end", "\n".repeat(100)));
        let source = "--!strict\n\nlocal make=require('./part')\n\nmake()";
        let project = Project::load(&fixture.0.join("main.luau"), source)?;
        let model = crate::model::compile_project_cached(
            source,
            Some(&project),
            &mut crate::analysis::Cache::default(),
        )?;
        assert_eq!(model.parts[0].line, 2);
        assert_eq!(model.parts[1].line, 4);
        Ok(())
    }

    #[test]
    fn unused_branches_and_functions_do_not_execute_imported_modules() -> Result<(), String> {
        let fixture = Fixture::new();
        fixture.write("unused.luau", "return error('unused module executed')");
        let source = "local active=false\nif active then local unused=require('./unused') end\nlocal function unused_factory() return require('./unused') end\ncad.show('safe',cad.box({width=2,depth=2,height=2}))";
        let project = Project::load(&fixture.0.join("main.luau"), source)?;
        let model = crate::model::compile_project_cached(
            source,
            Some(&project),
            &mut crate::analysis::Cache::default(),
        )?;
        assert_eq!(model.parts.len(), 1);
        assert_eq!(model.parts[0].name, "safe");
        assert_eq!(model.parts[0].line, 3);
        Ok(())
    }

    #[test]
    fn lazy_imports_share_singletons_and_preserve_actual_dependency_order() -> Result<(), String> {
        let fixture = Fixture::new();
        fixture.write("shared.luau", "return {count=0}");
        fixture.write(
            "first.luau",
            "local state=require('./shared'); state.count+=1; return state.count",
        );
        fixture.write(
            "second.luau",
            "local state=require('./shared'); state.count+=1; return state.count",
        );
        let source = "local deferred=function() return require('./first') end\nlocal second=require('./second')\nlocal first=deferred()\nlocal again=require('./first')\nassert(second==1); assert(first==2); assert(again==2)";
        let project = Project::load(&fixture.0.join("main.luau"), source)?;
        let lua = mlua::Lua::new();
        project.install(&lua).map_err(|error| error.to_string())?;
        lua.load(source)
            .set_name("@model.luau")
            .exec()
            .map_err(|error| error.to_string())?;
        Ok(())
    }

    #[test]
    fn failed_imports_leave_root_source_attribution_intact() -> Result<(), String> {
        let fixture = Fixture::new();
        fixture.write("bad.luau", "\n\n\nreturn error('expected failure')");
        let source = "local ok=pcall(function() return require('./bad') end)\nassert(not ok)\ncad.show('after',cad.box({width=2,depth=2,height=2}))";
        let project = Project::load(&fixture.0.join("main.luau"), source)?;
        let model = crate::model::compile_project_cached(
            source,
            Some(&project),
            &mut crate::analysis::Cache::default(),
        )?;
        assert_eq!(model.parts[0].name, "after");
        assert_eq!(model.parts[0].line, 2);
        Ok(())
    }

    #[test]
    fn large_standalone_comments_strings_and_identifiers_keep_the_editor_source_budget()
    -> Result<(), String> {
        for prefix in [
            "-- require\n",
            "local text='require'\n",
            "local requires_attention=true\n",
        ] {
            let source = format!("{prefix}{}", " ".repeat(SOURCE_BYTES_MAX));
            assert!(
                Project::load_if_imported(Path::new("/missing/project/main.luau"), &source)?
                    .is_none()
            );
        }
        let source = format!("{}require('./module')", " ".repeat(SOURCE_BYTES_MAX));
        assert!(
            Project::load_if_imported(Path::new("/missing/project/main.luau"), &source)
                .err()
                .expect("Project source budget")
                .contains("4 MiB")
        );
        Ok(())
    }
}
