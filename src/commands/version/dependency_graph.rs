/*
 * Part of this file is derived from the darklua project https://github.com/seaofvoices/darklua
 * which is licensed under the MIT License.
 *
 * Original Copyright (c) 2020 jeparlefrancais
 *
 * Permission is hereby granted, free of charge, to any person obtaining a copy
 * of this software and associated documentation files (the "Software"), to deal
 * in the Software without restriction, including without limitation the rights
 * to use, copy, modify, merge, publish, distribute, sublicense, and/or sell
 * copies of the Software, and to permit persons to whom the Software is
 * furnished to do so, subject to the following conditions:
 *
 * The above copyright notice and this permission notice shall be included in all
 * copies or substantial portions of the Software.
 *
 * THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
 * IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
 * FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
 * AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
 * LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
 * OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
 * SOFTWARE.
 */
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
};

use darklua_core::{
    process::{DefaultVisitor, NodeVisitor, ScopeVisitor},
    rules::{ContextBuilder, PathRequireMode, RequirePathLocator},
    Configuration, Resources,
};
use petgraph::algo::toposort;
use tracing::warn;

use crate::commands::version::{
    dependency_visitor::RequireDependencyProcessor,
    sdk_version::SdkVersionOut,
    utils::normalize_path,
    version_visitor::{VersionFile, VersionResolver},
};

#[derive(Default, Debug, Clone)]
pub enum State {
    #[default]
    NotProcessed,
    Processing,
    Processed,
}

#[derive(Default, Debug, Clone)]
pub struct DependencyGraphNode {
    depends_on: Vec<PathBuf>,
    /// it is a node/file that will be outputted (final flow)
    is_top_node: bool,
    sdk_version: SdkVersionOut,
    state: State,
    path: PathBuf,
    block: Option<darklua_core::nodes::Block>,
}

impl DependencyGraphNode {
    pub fn new(is_top_node: bool, path: PathBuf) -> Self {
        Self {
            is_top_node,
            path,
            ..Default::default()
        }
    }

    pub fn create_top_node(path: PathBuf) -> Self {
        DependencyGraphNode::new(true, path)
    }

    pub fn create_node(path: PathBuf) -> Self {
        DependencyGraphNode::new(false, path)
    }

    pub fn is_done(&self) -> bool {
        return matches!(self.state, State::Processed);
    }

    pub fn is_not_done(&self) -> bool {
        return !self.is_done();
    }
}

pub type DepedencyGraph = petgraph::stable_graph::StableDiGraph<DependencyGraphNode, ()>;

pub struct Work<'a> {
    pub graph: DepedencyGraph,
    node_mapping: HashMap<PathBuf, petgraph::stable_graph::NodeIndex>,
    resources: &'a Resources,
    configuration: Configuration,
    top_node_paths: Vec<PathBuf>,
    version_file: VersionFile,
}

impl<'a> Work<'a> {
    pub fn new(
        graph: DepedencyGraph,
        resources: &'a Resources,
        top_node_paths: Vec<PathBuf>,
        version_file: VersionFile,
    ) -> Self {
        Self {
            graph,
            node_mapping: HashMap::new(),
            resources,
            top_node_paths,
            configuration: Configuration::default(),
            version_file,
        }
    }

    /// Given a Vec<PathBuf>, create nodes for each dependency and add them to the graph
    /// Also, add everything to the node_mapping
    ///
    /// This does NOT add the edges itself
    fn add_dependencies_to_graph(
        &mut self,
        deps: Vec<PathBuf>,
    ) -> Vec<petgraph::stable_graph::NodeIndex> {
        deps.iter()
            .map(|dep| {
                let index = match self.node_mapping.get(dep) {
                    // if we already have the dependency cached, just return the index
                    Some(index) => index.clone(),
                    None => {
                        // if we don't have the dependency cached, create a new node and add it to the graph
                        let node = DependencyGraphNode::create_node(dep.clone());
                        let index = self.graph.add_node(node);
                        self.node_mapping.insert(dep.clone(), index);
                        index.clone()
                    }
                };

                index
            })
            .collect::<Vec<_>>()
    }

    fn advance_work(
        &mut self,
        node_index: petgraph::stable_graph::NodeIndex,
    ) -> anyhow::Result<State> {
        let state = match self.graph.node_weight_mut(node_index) {
            Some(node) => node.state.clone(),
            None => return Err(anyhow::anyhow!("Node not found")),
        };

        match state {
            State::NotProcessed => {
                // traverse and collect all the deps

                // we don't care about the parser retaining lines or being dense, just go with the default one
                let parser = darklua_core::Parser::default();

                let mut block = parser
                    .parse(
                        self.resources
                            .get(&self.get_node(node_index).path)
                            .map_err(|e| {
                                anyhow::anyhow!(
                                    "Failed to read file {}: {:?}",
                                    self.get_node(node_index).path.display(),
                                    e
                                )
                            })?
                            .as_str(),
                    )
                    .map_err(|e| {
                        anyhow::anyhow!(
                            "Failed to parse file {}: {:?}",
                            self.get_node(node_index).path.display(),
                            e
                        )
                    })?;

                let deps = self.collect_dependencies(node_index, &mut block)?.clone();

                self.add_dependencies_to_graph(deps.clone());

                let node = self.get_node_mut(node_index);
                node.state = State::Processing;
                node.depends_on = deps.clone();
                node.block = Some(block);
                Ok(State::Processing)
            }
            State::Processing => {
                // first, process the node's own sdk versions
                let mut version_visitor = VersionResolver::new(&self.version_file);

                let mut block = self
                    .graph
                    .node_weight_mut(node_index)
                    .unwrap()
                    .block
                    .as_mut()
                    .unwrap()
                    .clone();

                ScopeVisitor::visit_block(&mut block, &mut version_visitor);

                // process the node's data based on the deps AFTER we've collected all the nodes and added all the edges
                // otherwise, we'll get erroneous results

                self.get_node_mut(node_index).sdk_version = version_visitor.sdk_version();
                self.get_node_mut(node_index).state = State::Processed;
                Ok(State::Processed)
            }
            State::Processed => {
                // no work to do
                Ok(state)
            }
        }
    }

    fn get_node_mut(
        &mut self,
        node_index: petgraph::stable_graph::NodeIndex,
    ) -> &mut DependencyGraphNode {
        self.graph.node_weight_mut(node_index).unwrap()
    }

    fn get_node(&self, node_index: petgraph::stable_graph::NodeIndex) -> &DependencyGraphNode {
        self.graph.node_weight(node_index).unwrap()
    }

    fn create_rule_context<'block, 'src>(
        &self,
        source: &Path,
        original_code: &'src str,
    ) -> ContextBuilder<'block, 'a, 'src> {
        let builder = ContextBuilder::new(normalize_path(source), self.resources, original_code);
        if let Some(project_location) = self.configuration.location() {
            builder.with_project_location(project_location)
        } else {
            builder
        }
    }

    fn collect_dependencies(
        &mut self,
        node_index: petgraph::stable_graph::NodeIndex,
        block: &mut darklua_core::nodes::Block,
    ) -> anyhow::Result<Vec<PathBuf>> {
        // HARDCODED
        let context = self
            .create_rule_context(&self.graph.node_weight(node_index).unwrap().path, "")
            .build();
        let mut path_require_mode = PathRequireMode::default();
        path_require_mode
            .initialize(&context)
            .map_err(|e| anyhow::anyhow!("Failed to initialize path require mode: {:?}", e))?;

        let require_path_locator = RequirePathLocator::new(
            &path_require_mode,
            &self.get_node(node_index).path,
            &self.resources,
        );

        let mut visitor = RequireDependencyProcessor::new(
            self.get_node(node_index).path.clone(),
            require_path_locator,
        );

        DefaultVisitor::visit_block(block, &mut visitor);

        if visitor.errors().len() > 0 {
            return Err(anyhow::anyhow!(
                "Failed to collect dependencies: {:?}",
                visitor.errors()
            ));
        }

        Ok(visitor.deps().clone())
    }

    pub fn compute_dependency_graph(&mut self) -> Result<(), ()> {
        // normalize path
        // check to see if the nodes already exist in the graph
        // if they do, don't do anything
        // if they don't, create new nodes and add them to the graph
        // also, recursively compute the dependency graph for them if they don't exist
        for top_node_path in &self.top_node_paths {
            let path = normalize_path(top_node_path);
            if let Some(_) = self.node_mapping.get(&path) {
                continue;
            }

            let index = self
                .graph
                .add_node(DependencyGraphNode::create_top_node(path.clone()));
            self.node_mapping.insert(path, index);
        }

        let total_not_done = self
            .graph
            .node_weights()
            .filter(|work_item| !work_item.is_done())
            .count();

        if total_not_done == 0 {
            return Ok(());
        }

        let mut done_count = 0;

        'work_loop: loop {
            let mut add_edges = Vec::new();

            let node_indexes = match toposort(&self.graph, None) {
                Ok(node_indexes) => node_indexes.clone(),
                Err(err) => {
                    warn!("Error sorting graph, cycle detected: {:?}", err);
                    return Err(());
                }
            };

            for node_index in node_indexes {
                if self.get_node(node_index).is_not_done() {
                    match self.advance_work(node_index) {
                        Ok(State::NotProcessed) => unreachable!(),
                        Ok(State::Processing) => {
                            for dep in self.get_node(node_index).depends_on.clone() {
                                if let Some(content_node_index) = self.node_mapping.get(&dep) {
                                    add_edges.push((*content_node_index, node_index));
                                }
                            }
                        }
                        Ok(State::Processed) => {
                            // we have to get the sdk version of the node
                            done_count += 1;
                        }
                        Err(err) => {
                            warn!("Error advancing work: {:?}", err);
                            return Err(());
                        }
                    }
                }

                if done_count == self.graph.node_count() {
                    for (from, to) in add_edges {
                        self.graph.add_edge(from, to, ());
                    }
                    break 'work_loop;
                }
            }

            for (from, to) in add_edges {
                self.graph.add_edge(from, to, ());
            }
        }

        // now process the sdk versions based on the deps
        let node_indexes = match toposort(&self.graph, None) {
            Ok(node_indexes) => node_indexes.clone(),
            Err(err) => {
                warn!("Error sorting graph, cycle detected: {:?}", err);
                return Err(());
            }
        };

        for node_index in node_indexes {
            let sdk_version_of_deps = self
                .get_node(node_index)
                .depends_on
                .iter()
                .map(|dep| {
                    self.get_node(*self.node_mapping.get(dep).unwrap())
                        .sdk_version
                        .clone()
                })
                .collect::<Vec<_>>();

            self.get_node_mut(node_index).sdk_version = SdkVersionOut::sdk_version_intersection(
                self.get_node(node_index).sdk_version.clone(),
                sdk_version_of_deps
                    .iter()
                    .fold(SdkVersionOut::default(), |lhs, rhs| {
                        SdkVersionOut::sdk_version_intersection(lhs.clone(), rhs.clone())
                    }),
            );
        }

        Ok(())
    }

    pub fn get_versions(&self) -> HashMap<PathBuf, SdkVersionOut> {
        self.graph
            .node_weights()
            .filter(|node| node.is_top_node)
            .map(|node| (node.path.clone(), node.sdk_version.clone()))
            .collect()
    }

    #[allow(dead_code)]
    pub fn dot_graph(&self) -> String {
        use petgraph::dot::{Config, Dot};

        let dot_string = format!(
            "{:?}",
            Dot::with_attr_getters(
                &self.graph,
                &[Config::EdgeNoLabel, Config::NodeNoLabel],
                &|_, _| String::new(),
                &|_, (_, node)| {
                    format!(
                        "label=\"{} - {}\"",
                        node.path.display().to_string(),
                        node.sdk_version.min_sdk_version
                    )
                },
            )
        );
        dot_string
    }
}
