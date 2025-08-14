#!/bin/bash

function usage {
	printf "Usage: %s arg1 arg2\n" "$0"
	printf "  %-10s %s\n" "arg1" "An arbitrary parameter"
	printf "  %-10s %s\n" "arg2" "Another arbitrary parameter"
	exit 1
}

if [ $# -ne 2 ]; then
	usage
fi

echo -e "Generating that random number you asked for, $1"
echo $RANDOM
echo "Congratulations, that made me feel so $2"
