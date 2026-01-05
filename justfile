input := "/Users/oilcake/code/voop/smart_cut/samples/jt.mp4"
output := "/Users/oilcake/code/voop/smart_cut/output/cropped.mp4"
start := "55.0"
end := "68.7"

try:
	# cargo run -- --input {{input}} --start 55.0 --end 189.0
	cargo run -- --input {{input}} --output {{output}} --start {{start}} --end {{end}}
	mpv {{output}}
