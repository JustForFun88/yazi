local M = {}

function M:peek(job)
	local start = os.clock()
	ya.sleep(math.max(0, math.min(rt.preview.image_delay, 10) / 1000 + start - os.clock()))
	local _, err = ya.pdfium_pdf_page_show(job.file.url, job.skip, job.area)
	ya.preview_widget(job, err)
end

function M:seek(job)
	local h = cx.active.current.hovered
	if h and h.url == job.file.url then
		local step = ya.clamp(-1, job.units, 1)
		ya.emit("peek", { math.max(0, cx.active.preview.skip + step), only_if = job.file.url })
	end
end

function M:preload(job)
	local cache = ya.file_cache(job)
	if not cache or fs.cha(cache) then
		return true
	end

	return ya.pdfium_pdf_page_precache(job.file.url, job.skip, cache)
end

return M