# Werkflow

## Description

This is a work-in-progress workflow engine built in rust. The goals are to create a multi-threaded distributed system built in rust that has a large number of builtin facilities that can be used to automate tasks. I also want to promote security from the begining, with all sendpoints created by the agent automatically creating ssl certificates.

## Configuration

The default configuration language for werkflow is TOML, but I plan to support YAML and JSON in the future.

## Crates

| Name | Description | 
| ---- | ----------- |
| werkflow-agents | Library for building extensible agents. |
| werkflow-config | Library for shared configuration. |
| werkflow-datalayer | Datalayer bindings for caching, wide-column storage and rdbms access |
| werkflow-core | Core shared crate for all werkflow modules. Security helpers |
| werkflow-scripting | rhai scripting host for describing workflow tasks |
| werkflow-graph | *TODO* Graph helpers for building workflow graphs |