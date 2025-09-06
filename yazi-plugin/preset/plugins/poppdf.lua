local M = {}

function M:peek(job)
	local start = os.clock(),
	-- if not url or not fs.cha(url) then
	-- 	url = job.file.url
	-- end

	ya.sleep(math.max(0, rt.preview.image_delay / 1000 + start - os.clock()))
	ya.pdf_page_show(job.file.url.hhh, job.skip, job.area)
	ya.preview_widgets(job, {})
end

function M:seek(job)
	local h = cx.active.current.hovered
	if h and h.url == job.file.url then
		local step = ya.clamp(-1, job.units, 1)
		ya.mgr_emit("peek", { math.max(0, cx.active.preview.skip + step), only_if = job.file.url })
	end
end

function M:preload(job)
end

return M
