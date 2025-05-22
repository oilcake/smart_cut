input := '/Volumes/STUFF/Movies/Alita\ -\ Battle\ Angel_2019_WEB-DL\ 1080p.mkv'
output := '/Users/oilcake/code/voop/smart_cut/output/cropped.mp4'

try:
	cargo run -- --input {{input}} --start 55.0 --end 3589.0
	# cargo run -- --input {{input}} --output {{output}} --start 55.0 --end 56.0
