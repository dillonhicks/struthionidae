#!/usr/bin/env bash

main() {
    for x in $(echo "a b c d e f g h i j k l m n o p q r s t u v w x y z")
    do
	echo $x
	sleep 0.1s
    done 
}

main
