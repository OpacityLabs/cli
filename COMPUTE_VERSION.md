# Version Computation

This document describes the version computation process for Opacity CLI.

## Overview

The version computation process is responsible for computing the version of a flow based on its dependencies.

## Process

1. Compute the dependency graph
2. Compute the version of each node based on itself directly (no deps), while also collecting all the nodes and adding all the edges
3. Compute the version of each node based on its dependencies
4. Return the versions of all the top nodes

## Dependency Graph

The dependency graph is a directed graph where each node represents a flow and each edge represents a dependency between two flows.

## Top Nodes

The top nodes are the nodes that will be outputted (final flows).

## Per-File Version Computation

1. Parse the file
2. Call the VersionResolver to comptue the version of the file
	- This visitor keeps track of the scope
	- It takes into consideration calling of functions using `pcall`'s
	- It takes into consideration SIMPLE if/else statements (only one if branch, not multiple elseifs) where there is a reference to the `sdk_version_function` (the function that tells us the current sdk version at runtime)
	```lua
	if fetch_sdk_version() > 25 then
		use_function_min_sdk_version_26() -- min: 26, max: None
	else
		use_function_min_sdk_version_23() -- min: 23, max: Some(25)
	end

	-- after processing, the version of the file will be the minimum of the 2 versions
	```
	- The call to the `sdk_version_function` should NOT be done with the pcall function, it should be done directly

## Shortcomings

- At the moment, we compute the SDK Version PER FILE, NOT PER FUNCTION! In the future, we plan to compute the SDK Version PER FUNCTION, take into account exported functions (if it is an object, its methods) and then when AND IF they are used, then we will act accordingly

## Requests to Devs

When updating the SDK and adding a new function, make sure to increase the version of the SDK and add the new functions to the `version_file.json` file in the flows' folder