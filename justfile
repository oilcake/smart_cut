input := "/Users/oilcake/code/voop/samples/jt.ts"
output := "/Users/oilcake/code/voop/smart_cut/output/cropped.ts"
start := "55.0"
end := "189.0"

try:
	# cargo run -- --input {{input}} --start 55.0 --end 189.0
	cargo run -- --input {{input}} --output {{output}} --start {{start}} --end {{end}}
