#!/bin/bash

docker run -p 9042:9042 --name $ARGV[1] --hostname $ARGV[1] -d scylladb/scylla